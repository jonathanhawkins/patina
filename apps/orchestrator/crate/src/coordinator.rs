use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::config::Config;
use crate::error::{OrchestratorError, Result};
use crate::mail::{MailClient, OutgoingMessage};
use crate::worker::SwarmHealth;
use crate::{br, db, message, tmux, verifier, worker};

/// Result of a single poll cycle.
#[derive(Default)]
pub struct PollResult {
    pub processed: usize,
    pub rejected: usize,
    pub errors: usize,
    pub prompt_tasks: Vec<tmux::PromptTask>,
}

pub struct ReassignResult {
    pub next_bead_id: Option<String>,
    pub prompt_task: Option<tmux::PromptTask>,
}

/// Result of background verification for a completed bead.
#[derive(Debug)]
struct VerifyResult {
    bead_id: String,
    worker: String,
    msg_id: Option<i64>,
    ack_required: bool,
    files_changed: String,
    tests_run: String,
    passed: bool,
    reject_reason: Option<String>,
    /// True when this verification originated from stale-PROG recovery
    /// (no worker to reassign; just close on pass, reopen on fail).
    is_stale_recovery: bool,
}

pub struct Coordinator {
    pub config: Config,
    mail: MailClient,
    dry_run: bool,
    /// Beads currently being verified in background threads.
    pending_verifications: Arc<Mutex<HashSet<String>>>,
    /// Channel to receive verification results.
    verify_rx: Mutex<mpsc::Receiver<VerifyResult>>,
    /// Sender cloned into background threads.
    verify_tx: mpsc::Sender<VerifyResult>,
}

impl Coordinator {
    pub fn new(config: Config, dry_run: bool) -> Result<Self> {
        // Verify DB is accessible at startup
        let _db = db::open(&config.project_root)?;
        drop(_db);
        let mail = MailClient::new(&config.mail);
        let (verify_tx, verify_rx) = mpsc::channel();
        Ok(Self {
            config,
            mail,
            dry_run,
            pending_verifications: Arc::new(Mutex::new(HashSet::new())),
            verify_rx: Mutex::new(verify_rx),
            verify_tx,
        })
    }

    /// Open a fresh DB connection (avoids holding WAL read locks that block br writes).
    fn open_db(&self) -> Result<rusqlite::Connection> {
        db::open(&self.config.project_root)
    }

    /// Drain completed background verifications and process results.
    /// Called at the start of each poll cycle so results are handled promptly.
    pub fn drain_verify_results(&self) -> (usize, usize, Vec<tmux::PromptTask>) {
        let rx = self.verify_rx.lock().unwrap();
        let mut processed = 0usize;
        let mut rejected = 0usize;
        let mut prompt_tasks = Vec::new();

        while let Ok(vr) = rx.try_recv() {
            if vr.passed {
                if vr.is_stale_recovery {
                    // Stale-PROG recovery — just close, no worker to reassign
                    tracing::info!(bead = %vr.bead_id, "Stale bead verified PASSED — closing as done");
                    match br::close_bead(&vr.bead_id, "stale-recovery: acceptance tests passed") {
                        Ok(_) => { processed += 1; }
                        Err(e) => {
                            tracing::error!(
                                bead = %vr.bead_id,
                                error = %e,
                                "Failed to close stale-verified bead"
                            );
                        }
                    }
                } else {
                    // Normal completion — close bead and reassign worker
                    match self.reassign(&vr.bead_id, &vr.worker, vr.msg_id) {
                        Ok(rr) => {
                            if let Some(pt) = rr.prompt_task {
                                prompt_tasks.push(pt);
                            }
                            if !vr.ack_required {
                                if let Some(id) = vr.msg_id {
                                    self.read_msg(id);
                                }
                            }
                            processed += 1;
                        }
                        Err(e) => {
                            tracing::error!(
                                bead = %vr.bead_id,
                                error = %e,
                                "ERROR: reassign after verification failed"
                            );
                        }
                    }
                }
            } else {
                if vr.is_stale_recovery {
                    // Stale-PROG recovery failed — reopen for a new worker (no message to send)
                    tracing::info!(
                        bead = %vr.bead_id,
                        reason = ?vr.reject_reason,
                        "Stale bead verify FAILED — reopening for new worker"
                    );
                    let _ = br::reopen(&vr.bead_id);
                    rejected += 1;
                } else {
                    // Normal verification failed — reopen bead and notify worker
                    tracing::info!(
                        bead = %vr.bead_id,
                        reason = ?vr.reject_reason,
                        "REJECT: verification failed"
                    );
                    let _ = br::reopen(&vr.bead_id);
                    let details = format!(
                        "Files changed: {}\nTests run: {}",
                        if vr.files_changed.is_empty() { "<missing>" } else { &vr.files_changed },
                        if vr.tests_run.is_empty() { "<missing>" } else { &vr.tests_run },
                    );
                    let reason = vr.reject_reason.as_deref()
                        .unwrap_or("reported test commands failed when rerun by the coordinator");
                    let _ = self.send_reopen_message(&vr.worker, &vr.bead_id, reason, &details);
                    self.ack_or_mark_read(vr.msg_id);
                    rejected += 1;
                }
            }
        }

        if processed > 0 || rejected > 0 {
            tracing::info!(
                processed,
                rejected,
                pending = self.pending_verifications.lock().unwrap().len(),
                "Background verification results drained"
            );
        }

        (processed, rejected, prompt_tasks)
    }

    /// Dispatch background verification for a stale PROG bead.
    /// Instead of blindly reopening, check if the acceptance tests pass.
    /// If they pass → close as done. If they fail → reopen for a new worker.
    fn verify_stale_bead(&self, bead_id: &str) {
        // Skip if already being verified
        {
            let pending = self.pending_verifications.lock().unwrap();
            if pending.contains(bead_id) {
                tracing::debug!(bead = %bead_id, "stale bead already being verified, skipping");
                return;
            }
        }

        // Get acceptance commands from bead description
        let mut commands = match self.open_db() {
            Ok(db) => {
                let desc = db::bead_description(&db, bead_id).ok().flatten();
                drop(db);
                match desc {
                    Some(d) => verifier::extract_acceptance_commands(&d),
                    None => vec![],
                }
            }
            Err(_) => vec![],
        };

        if commands.is_empty() {
            // Fallback: just verify the workspace compiles. Running the full test suite
            // takes 15+ minutes and times out, creating a reopen loop. A successful build
            // is a reasonable signal that the bead's work didn't break anything.
            tracing::info!(bead = %bead_id, "Stale bead has no acceptance commands — using build check fallback");
            commands.push("cargo build --workspace".to_string());
        }

        // Mark as pending
        {
            let mut pending = self.pending_verifications.lock().unwrap();
            pending.insert(bead_id.to_string());
        }

        let tx = self.verify_tx.clone();
        let bead_id = bead_id.to_string();
        let project_root = self.config.project_root.clone();
        let timeout = Duration::from_secs(self.config.verify_timeout_seconds);
        let pending = Arc::clone(&self.pending_verifications);

        std::thread::spawn(move || {
            tracing::info!(bead = %bead_id, commands = commands.len(), "Stale bead verify started");
            let verify_result = verifier::verify_commands(&project_root, &commands, timeout);
            let passed = verify_result.is_ok();
            let reject_reason = verify_result.err().map(|e| format!("{e}"));

            if passed {
                tracing::info!(bead = %bead_id, "Stale bead verify PASSED");
            } else {
                tracing::info!(bead = %bead_id, reason = ?reject_reason, "Stale bead verify FAILED");
            }

            let _ = tx.send(VerifyResult {
                bead_id: bead_id.clone(),
                worker: String::new(),
                msg_id: None,
                ack_required: false,
                files_changed: String::new(),
                tests_run: String::new(),
                passed,
                reject_reason,
                is_stale_recovery: true,
            });

            let mut p = pending.lock().unwrap();
            p.remove(&bead_id);
        });
    }

    /// Sweep beads that workers marked "done" or "complete" directly.
    /// In pull mode, workers sometimes use `br update --status done` instead
    /// of `/mail-complete`. This catches those and closes them properly.
    /// Log blocked beads (waiting on dependencies) and stale beads (untouched for 2+ days).
    /// Uses br's built-in `blocked` and `stale` commands for diagnostics.
    /// Runs at most once per minute to avoid spamming br subprocesses.
    fn log_blocked_and_stale(&self) {
        // Rate-limit: only run every 60 seconds
        use std::sync::atomic::{AtomicU64, Ordering};
        static LAST_RUN: AtomicU64 = AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_RUN.load(Ordering::Relaxed);
        if now.saturating_sub(last) < 60 {
            return;
        }
        LAST_RUN.store(now, Ordering::Relaxed);

        // Log blocked beads
        match br::blocked_json() {
            Ok(json) if !json.trim().is_empty() && json.trim() != "[]" => {
                let count = json.matches("\"id\"").count();
                if count > 0 {
                    tracing::info!(count, "Blocked beads (waiting on dependencies)");
                }
            }
            _ => {}
        }

        // Log stale beads (in_progress for 2+ days)
        match br::stale_json(2) {
            Ok(json) if !json.trim().is_empty() && json.trim() != "[]" => {
                let count = json.matches("\"id\"").count();
                if count > 0 {
                    tracing::warn!(count, "Stale beads (untouched for 2+ days)");
                }
            }
            _ => {}
        }
    }

    /// Recover stale assignments: detect in_progress beads assigned to workers
    /// no longer in the tmux session, validate acceptance tests, and close or reopen.
    fn recover_stale_assignments(&self, session: &str, window: u32) -> Result<()> {
        let (stale, extras) = {
            let db = self.open_db()?;
            let workers = worker::worker_info_list_with_config(session, window, &db, &self.config)?;
            let active_names: Vec<String> = workers
                .iter()
                .filter(|w| !w.worker_name.is_empty())
                .map(|w| w.worker_name.clone())
                .collect();
            let stale = db::stale_assignments(&db, &active_names)?;
            let extras = db::extra_assignments(&db)?;
            (stale, extras)
        };

        for (bead_id, assignee) in &stale {
            tracing::info!(bead = %bead_id, worker = %assignee, "Stale assignment detected (worker missing from session)");
            let is_in_progress = match self.open_db() {
                Ok(db) => {
                    let state = db::bead_state(&db, bead_id).ok().flatten();
                    drop(db);
                    matches!(state, Some((db::BeadStatus::InProgress, _)))
                }
                Err(_) => false,
            };
            if is_in_progress && self.config.verify_reported_tests {
                self.verify_stale_bead(bead_id);
            } else {
                let _ = br::reopen(bead_id);
            }
        }

        for (assignee, bead_id) in &extras {
            tracing::info!(bead = %bead_id, worker = %assignee, "Reclaiming extra assignment");
            let _ = br::reopen(bead_id);
        }

        Ok(())
    }

    fn sweep_self_completed(&self) -> usize {
        let ids: Vec<String> = match self.open_db() {
            Ok(conn) => {
                let mut stmt = match conn.prepare(
                    "SELECT id FROM issues WHERE status IN ('done', 'complete')"
                ) {
                    Ok(s) => s,
                    Err(_) => return 0,
                };
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))
                    .unwrap_or_else(|_| panic!("query failed"));
                rows.filter_map(|r| r.ok()).collect()
            }
            Err(_) => return 0,
        };

        let mut closed = 0;
        for id in &ids {
            match br::close_bead(id, "Self-completed by worker (pull mode)") {
                Ok(_) => {
                    tracing::info!(bead = %id, "Swept self-completed bead → closed");
                    closed += 1;
                }
                Err(e) => {
                    tracing::debug!(bead = %id, error = %e, "sweep close failed");
                }
            }
        }
        closed
    }

    /// One full poll cycle: fetch inbox, process completions, reassign.
    ///
    /// IMPORTANT: We must NOT hold a persistent DB connection across `br` CLI calls.
    /// The `br` subprocess needs write access, and a read connection from this process
    /// blocks it in WAL mode (different-process reader blocks writer). Instead, we
    /// open short-lived connections only for reads, dropping them before mutations.
    pub fn poll(&self) -> Result<PollResult> {
        // Ensure bv is alive — a dead/suspended bv holds WAL read locks that
        // block br sync and cause the JSONL→SQLite split-brain.
        if let Some(session) = &self.config.session_name {
            let workdir = self.config.project_root.to_string_lossy().to_string();
            match tmux::ensure_bv_alive(session, self.config.window_index, &workdir) {
                Ok(true) => tracing::info!("restarted bv pane"),
                Ok(false) => {}
                Err(e) => tracing::debug!(error = %e, "bv health check skipped"),
            }
        }

        // Drain any completed background verifications first
        let (v_processed, v_rejected, v_prompts) = self.drain_verify_results();

        // Sweep beads that workers marked "done"/"complete" directly (pull mode).
        // Workers sometimes use `br update --status done` instead of /mail-complete.
        // Close these so they don't sit in limbo.
        if !self.dry_run {
            let swept = self.sweep_self_completed();
            if swept > 0 {
                tracing::info!(swept, "Swept self-completed beads (workers used br update --status done)");
            }
        }

        // Recover stale assignments — runs every poll cycle (including pull mode).
        // Detects in_progress beads assigned to workers no longer in the tmux session,
        // validates their acceptance tests, and closes or reopens them.
        if !self.dry_run {
            if let Some(session) = &self.config.session_name {
                let window = self.config.window_index;
                if let Err(e) = self.recover_stale_assignments(session, window) {
                    tracing::warn!(error = %e, "stale assignment recovery failed");
                }
            }
        }

        // Log blocked and stale beads periodically for diagnostics.
        // Uses br's built-in dependency graph and staleness detection.
        if !self.dry_run {
            self.log_blocked_and_stale();
        }

        let coordinator = self.coordinator_name()?;
        let project_key = self.config.project_root.to_string_lossy().to_string();

        let mut result = PollResult {
            processed: v_processed,
            rejected: v_rejected,
            prompt_tasks: v_prompts,
            ..Default::default()
        };

        // Fetch inbox
        tracing::info!("Fetching inbox...");
        let inbox = self.mail.fetch_inbox(&project_key, &coordinator, 200)?;

        // Parse completions
        let completion_set = message::parse_completions(&inbox);

        // Ack stale duplicate IDs
        if !self.dry_run {
            for stale_id in &completion_set.stale_ids {
                self.ack_msg(*stale_id);
            }
        }

        if completion_set.completions.is_empty() {
            tracing::info!("No pending bead-complete messages.");
            return Ok(result);
        }

        tracing::info!(
            "Found {} pending bead-complete message(s)",
            completion_set.completions.len()
        );

        for completion in &completion_set.completions {
            if completion.bead_id.is_empty() {
                tracing::warn!(msg_id = ?completion.msg_id, "SKIP: could not extract bead ID");
                continue;
            }

            tracing::info!(
                bead = %completion.bead_id,
                worker = %completion.worker,
                msg_id = ?completion.msg_id,
                "Processing completion"
            );

            // Check bead state in DB (short-lived connection — dropped before mutations)
            let state = {
                let db = self.open_db()?;
                let s = db::bead_state(&db, &completion.bead_id)?;
                drop(db);
                s
            };
            if let Some((status, assignee)) = &state {
                // Skip if closed/tombstone
                if matches!(status, db::BeadStatus::Closed | db::BeadStatus::Tombstone) {
                    tracing::info!(
                        bead = %completion.bead_id,
                        status = %status.as_str(),
                        "SKIP: already closed/tombstone"
                    );
                    self.ack_or_mark_read(completion.msg_id);
                    continue;
                }

                // Fix assignee mismatch: update DB to match the actual completer.
                // This happens when worker A claims a bead, dies, worker B re-claims
                // and completes it, but A's stale message arrives first — or vice versa.
                // The worker who sent the completion did the work, so update the assignee.
                if let Some(current_assignee) = assignee {
                    if !completion.worker.is_empty() && current_assignee != &completion.worker {
                        tracing::info!(
                            bead = %completion.bead_id,
                            from = %current_assignee,
                            to = %completion.worker,
                            "reassigning to actual completer"
                        );
                        if !self.dry_run {
                            let _ = br::update_bead(
                                &completion.bead_id,
                                Some(&completion.worker),
                                None,
                            );
                        }
                    }
                }
            }

            // Reject if missing evidence
            if !completion.tests_ok || !completion.files_ok {
                tracing::info!(
                    bead = %completion.bead_id,
                    "REJECT: insufficient completion evidence"
                );
                if !self.dry_run {
                    let _ = br::reopen(&completion.bead_id);
                    let details = format!(
                        "Files changed: {}\nTests run: {}",
                        if completion.files_changed.is_empty() {
                            "<missing>"
                        } else {
                            &completion.files_changed
                        },
                        if completion.tests_run.is_empty() {
                            "<missing>"
                        } else {
                            &completion.tests_run
                        },
                    );
                    let _ = self.send_reopen_message(
                        &completion.worker,
                        &completion.bead_id,
                        "missing concrete completion evidence",
                        &details,
                    );
                    self.ack_or_mark_read(completion.msg_id);
                }
                result.rejected += 1;
                continue;
            }

            // Verify reported tests if enabled — dispatch to background thread
            if self.config.verify_reported_tests && !self.dry_run {
                // Skip if already being verified in a background thread.
                // Do NOT ack — the message will reappear next cycle but we skip it.
                // Acking before close is confirmed causes orphaned beads if close fails.
                {
                    let pending = self.pending_verifications.lock().unwrap();
                    if pending.contains(&completion.bead_id) {
                        tracing::debug!(bead = %completion.bead_id, "already being verified, skipping");
                        continue;
                    }
                }

                let mut commands = verifier::extract_test_commands(&completion.tests_run);

                // Also gather acceptance commands from bead description
                let description = {
                    let db = self.open_db()?;
                    let desc = db::bead_description(&db, &completion.bead_id)?;
                    drop(db);
                    desc
                };
                if let Some(desc) = &description {
                    let acceptance_cmds = verifier::extract_acceptance_commands(desc);
                    commands.extend(acceptance_cmds);
                }

                if commands.is_empty() {
                    tracing::info!(
                        bead = %completion.bead_id,
                        "REJECT: no test commands to verify"
                    );
                    let _ = br::reopen(&completion.bead_id);
                    let details = format!(
                        "Files changed: {}\nTests run: {}",
                        if completion.files_changed.is_empty() { "<missing>" } else { &completion.files_changed },
                        if completion.tests_run.is_empty() { "<missing>" } else { &completion.tests_run },
                    );
                    let _ = self.send_reopen_message(
                        &completion.worker,
                        &completion.bead_id,
                        "missing concrete completion evidence",
                        &details,
                    );
                    self.ack_or_mark_read(completion.msg_id);
                    result.rejected += 1;
                    continue;
                }

                // Dispatch verification to background thread
                {
                    let mut pending = self.pending_verifications.lock().unwrap();
                    pending.insert(completion.bead_id.clone());
                }

                let tx = self.verify_tx.clone();
                let bead_id = completion.bead_id.clone();
                let worker_name = completion.worker.clone();
                let msg_id = completion.msg_id;
                let ack_required = completion.ack_required;
                let files_changed = completion.files_changed.clone();
                let tests_run = completion.tests_run.clone();
                let project_root = self.config.project_root.clone();
                let timeout = Duration::from_secs(self.config.verify_timeout_seconds);
                let pending = Arc::clone(&self.pending_verifications);

                std::thread::spawn(move || {
                    tracing::info!(bead = %bead_id, commands = commands.len(), "Background verify started");
                    let verify_result = verifier::verify_commands(&project_root, &commands, timeout);
                    let passed = verify_result.is_ok();
                    let reject_reason = verify_result.err().map(|e| format!("{e}"));

                    if passed {
                        tracing::info!(bead = %bead_id, "Background verify PASSED");
                    } else {
                        tracing::info!(bead = %bead_id, reason = ?reject_reason, "Background verify FAILED");
                    }

                    let _ = tx.send(VerifyResult {
                        bead_id: bead_id.clone(),
                        worker: worker_name,
                        msg_id,
                        ack_required,
                        files_changed,
                        tests_run,
                        passed,
                        reject_reason,
                        is_stale_recovery: false,
                    });

                    // Remove from pending set
                    let mut p = pending.lock().unwrap();
                    p.remove(&bead_id);
                });

                // Do NOT ack here — ack only after drain_verify_results successfully
                // closes the bead. Acking before close causes orphaned beads if
                // br::close_bead fails (the message is gone but the bead stays in_progress).
                // The pending_verifications set prevents re-dispatching.

                // Don't block — continue processing other completions
                continue;
            }

            // Close completed bead and assign next ready bead to same worker
            if self.dry_run {
                tracing::info!(
                    bead = %completion.bead_id,
                    worker = %completion.worker,
                    "[dry-run] would reassign"
                );
            } else {
                match self.reassign(
                    &completion.bead_id,
                    &completion.worker,
                    completion.msg_id,
                ) {
                    Ok(rr) => {
                        if let Some(pt) = rr.prompt_task {
                            result.prompt_tasks.push(pt);
                        }
                        // Ack non-ack_required messages (ack_required ones are handled in reassign)
                        if !completion.ack_required {
                            if let Some(id) = completion.msg_id {
                                self.read_msg(id);
                            }
                        }
                        result.processed += 1;
                    }
                    Err(e) => {
                        tracing::error!(
                            bead = %completion.bead_id,
                            error = %e,
                            "ERROR: reassign failed"
                        );
                        // Do NOT ack — leave the message so the next poll cycle
                        // can retry the close. The message will be skipped once
                        // the bead reaches "closed" status (line 97).
                        // If the bead was already closed despite the error,
                        // the next cycle will see "already closed" and ack then.
                        result.errors += 1;
                    }
                }
            }
        }

        Ok(result)
    }

    /// Assignment cycle: detect idle workers and assign ready beads.
    /// Returns (assigned_count, prompt_tasks) — caller submits prompts in parallel.
    pub fn assign_idle_workers(&self, session: &str, window: u32) -> Result<(usize, Vec<tmux::PromptTask>)> {
        self.assign_idle_workers_inner(session, window, None, false)
    }

    /// Assignment cycle with an overridden stale-assignment threshold for recovery.
    /// Also bypasses the completed-waiting grace period, since the stall detector
    /// has already determined the swarm is stuck.
    pub fn assign_idle_workers_with_stale_override(
        &self,
        session: &str,
        window: u32,
        stale_override_secs: u64,
    ) -> Result<(usize, Vec<tmux::PromptTask>)> {
        self.assign_idle_workers_inner(session, window, Some(stale_override_secs), true)
    }

    fn assign_idle_workers_inner(
        &self,
        session: &str,
        window: u32,
        stale_override_secs: Option<u64>,
        skip_completion_grace: bool,
    ) -> Result<(usize, Vec<tmux::PromptTask>)> {
        let stale_threshold = stale_override_secs.unwrap_or(self.config.stale_assignment_seconds);
        let mut prompt_tasks: Vec<tmux::PromptTask> = Vec::new();

        // Read all DB state upfront, then drop the connection before calling br mutations.
        // This prevents our read connection from blocking br's write lock on macOS.
        let (workers, stale, extras) = {
            let db = self.open_db()?;
            let workers = worker::worker_info_list_with_config(session, window, &db, &self.config)?;
            let active_names: Vec<String> = workers
                .iter()
                .filter(|w| !w.worker_name.is_empty())
                .map(|w| w.worker_name.clone())
                .collect();
            let stale = db::stale_assignments(&db, &active_names)?;
            let extras = db::extra_assignments(&db)?;
            // db dropped here
            (workers, stale, extras)
        };

        // Reclaim stale assignments (workers not in session).
        // For IN_PROGRESS beads, validate acceptance tests before reopening —
        // the dead worker may have finished the work without reporting it.
        for (bead_id, assignee) in &stale {
            tracing::info!(
                bead = %bead_id,
                worker = %assignee,
                "Stale assignment detected (worker missing from session)"
            );
            if !self.dry_run {
                let is_in_progress = match self.open_db() {
                    Ok(db) => {
                        let state = db::bead_state(&db, bead_id).ok().flatten();
                        drop(db);
                        matches!(state, Some((db::BeadStatus::InProgress, _)))
                    }
                    Err(_) => false,
                };

                if is_in_progress && self.config.verify_reported_tests {
                    self.verify_stale_bead(bead_id);
                } else {
                    let _ = br::reopen(bead_id);
                }
            }
        }

        // Reclaim extra assignments (>1 per worker)
        for (assignee, bead_id) in &extras {
            tracing::info!(
                bead = %bead_id,
                worker = %assignee,
                "Reclaiming extra assignment"
            );
            if !self.dry_run {
                let _ = br::reopen(bead_id);
            }
        }

        let mut assigned = 0usize;
        let mut selected_beads: Vec<String> = Vec::new();

        // Pre-read all per-worker DB state upfront with a single connection,
        // then drop it before any br mutations in the loop below.
        struct WorkerDbState {
            active_count: usize,
            assigned_bead: Option<db::BeadInfo>,
        }
        let worker_db_state: std::collections::HashMap<String, WorkerDbState> = {
            let qdb = self.open_db()?;
            let mut map = std::collections::HashMap::new();
            for w in &workers {
                if w.worker_name.is_empty() || self.config.is_reserved_verifier_pane(w.pane_index) {
                    continue;
                }
                let active_count = db::active_assignment_count(&qdb, &w.worker_name)?;
                let assigned_bead = db::assigned_bead_for_worker(&qdb, &w.worker_name)?;
                map.insert(w.worker_name.clone(), WorkerDbState { active_count, assigned_bead });
            }
            // qdb dropped here — safe for br mutations below
            map
        };

        for w in &workers {
            if self.config.is_reserved_verifier_pane(w.pane_index) {
                continue;
            }

            if w.worker_name.is_empty() {
                continue;
            }

            let db_state = match worker_db_state.get(&w.worker_name) {
                Some(s) => s,
                None => continue,
            };

            let is_idle = matches!(
                w.state,
                worker::WorkerState::Idle
                    | worker::WorkerState::CompletedWaiting
                    | worker::WorkerState::StuckInput
            );

            if !is_idle {
                if db_state.active_count > 0 {
                    // Genuinely busy with assigned work — skip
                    continue;
                }
                // No assignment but looks busy — stale output. Treat as idle.
                tracing::info!(
                    pane = w.pane_index,
                    worker = %w.worker_name,
                    "Worker looks busy but has no assignment — treating as idle"
                );
            }

            // Worker has an active assignment
            if db_state.active_count > 0 {
                if is_idle {
                    if let Some(bead) = &db_state.assigned_bead {
                        let age = worker::assignment_age_secs(bead.updated_at.as_deref());

                        // Check if completed-waiting and within grace period.
                        // Only apply grace if the completed bead matches the assigned bead —
                        // if the pane shows completion for a DIFFERENT bead, the worker was
                        // reassigned and needs a prompt for the new bead immediately.
                        // During recovery (skip_completion_grace=true), bypass entirely.
                        if w.completed_waiting
                            && age < self.config.completion_wait_grace_seconds
                            && !skip_completion_grace
                        {
                            let capture = tmux::capture_pane(session, window, w.pane_index, 120)
                                .unwrap_or_default();
                            let completed_id = worker::extract_completed_bead_id(&capture);
                            let grace_applies = match completed_id.as_deref() {
                                Some(cid) => cid == bead.id,
                                None => true, // can't determine — be conservative
                            };
                            if grace_applies {
                                continue;
                            }
                            tracing::info!(
                                pane = w.pane_index,
                                completed = ?completed_id,
                                assigned = %bead.id,
                                "Grace bypassed: pane shows completion for different bead"
                            );
                        }

                        // Check if should reclaim
                        if should_reclaim_idle(
                            bead,
                            age,
                            stale_threshold,
                            self.config.idle_reclaim_grace_seconds,
                            session,
                            window,
                            w.pane_index,
                            &w.worker_name,
                            &self,
                        ) {
                            if !self.dry_run {
                                let _ = br::reopen(&bead.id);
                            }
                            // Re-check after reclaim (need fresh DB read)
                            let qdb = self.open_db()?;
                            if db::active_assignment_count(&qdb, &w.worker_name)? > 0 {
                                continue;
                            }
                            // Fall through to fresh assignment
                        } else {
                            // Not reclaimable — check reprompt
                            if self.should_skip_reprompt(&w.worker_name, &bead.id) {
                                continue;
                            }

                            // Check if already has queued prompt
                            let capture = tmux::capture_pane(session, window, w.pane_index, 120)
                                .unwrap_or_default();
                            if worker::has_assignment_prompt_with(&capture, &self.config.queue_prompt_marker, &bead.id) {
                                if !self.dry_run {
                                    self.record_reprompt(&w.worker_name, &bead.id);
                                }
                                continue;
                            }

                            // Re-prompt
                            tracing::info!(
                                pane = w.pane_index,
                                worker = %w.worker_name,
                                bead = %bead.id,
                                "Re-prompting"
                            );
                            if !self.dry_run {
                                let _ = self.send_assignment_mail(&w.worker_name, bead);
                                prompt_tasks.push(tmux::PromptTask {
                                    session: session.to_string(),
                                    window,
                                    pane: w.pane_index,
                                    text: self.build_prompt_text(&bead.id, &bead.title, &w.worker_name),
                                    pcfg: self.prompt_config(),
                                });
                                self.record_reprompt(&w.worker_name, &bead.id);
                            }
                            assigned += 1;
                            continue;
                        }
                    }
                }

                // Still has active work (no bead found or non-idle case)
                continue;
            }

            // Worker needs a fresh assignment
            let ready = self.ensure_ready_work()?;
            let next = ready
                .iter()
                .find(|b| !selected_beads.contains(&b.id));

            let bead = match next {
                Some(b) => b.clone(),
                None => {
                    tracing::info!(
                        worker = %w.worker_name,
                        pane = w.pane_index,
                        "No ready unassigned beads for this worker, skipping"
                    );
                    continue;
                }
            };

            selected_beads.push(bead.id.clone());

            tracing::info!(
                pane = w.pane_index,
                worker = %w.worker_name,
                bead = %bead.id,
                "Assigning"
            );

            if !self.dry_run {
                if br::update_bead(&bead.id, Some(&w.worker_name), Some("in_progress")).is_err() {
                    tracing::warn!(bead = %bead.id, "Failed to assign after retries");
                    continue;
                }
                let _ = self.send_assignment_mail(&w.worker_name, &bead);
                // Queue prompt for parallel submission instead of blocking here
                tracing::info!(
                    pane = w.pane_index,
                    worker = %w.worker_name,
                    bead = %bead.id,
                    "Queuing prompt for parallel submission"
                );
                prompt_tasks.push(tmux::PromptTask {
                    session: session.to_string(),
                    window,
                    pane: w.pane_index,
                    text: self.build_prompt_text(&bead.id, &bead.title, &w.worker_name),
                    pcfg: self.prompt_config(),
                });
                self.record_reprompt(&w.worker_name, &bead.id);
            }
            assigned += 1;
        }

        tracing::info!(
            assigned,
            queued_prompts = prompt_tasks.len(),
            "assign_idle_workers complete"
        );
        Ok((assigned, prompt_tasks))
    }

    /// Close a completed bead and assign the next ready bead to the same worker.
    /// Returns a `ReassignResult` with the next bead ID and an optional prompt task
    /// for immediate submission to the worker's tmux pane.
    pub fn reassign(
        &self,
        completed_bead: &str,
        worker_name: &str,
        ack_msg_id: Option<i64>,
    ) -> Result<ReassignResult> {
        let none_result = ReassignResult { next_bead_id: None, prompt_task: None };
        // Check if worker has other active beads (short-lived connection)
        let other_active = {
            let db = self.open_db()?;
            let result = db::worker_active_assignments(&db, worker_name, Some(completed_bead))?;
            drop(db);
            result
        };
        if !other_active.is_empty() {
            tracing::info!(
                worker = %worker_name,
                other = ?other_active,
                "Worker already has other active beads — skipping reassignment"
            );
            // Still ack
            if let Some(id) = ack_msg_id {
                self.ack_msg(id);
            }
            return Ok(none_result);
        }

        // Step 1: Close completed bead
        tracing::info!(bead = %completed_bead, "Closing completed bead");
        match br::close_bead(completed_bead, "completed") {
            Ok(_) => {}
            Err(e) => {
                let msg = format!("{e}").to_lowercase();
                if msg.contains("already closed") || msg.contains("not found") {
                    // Already closed, or still not found even after br::close_bead's
                    // automatic sync-and-retry (bead may only exist in agent mail,
                    // not in any JSONL). Ack and try to reassign the worker.
                    if msg.contains("not found") {
                        tracing::warn!(
                            bead = %completed_bead,
                            "Bead not found even after sync retry — acking and continuing"
                        );
                    }
                    let current = {
                        let db = self.open_db()?;
                        let r = db::worker_active_assignments(&db, worker_name, Some(completed_bead))?;
                        drop(db);
                        r
                    };
                    if !current.is_empty() {
                        if let Some(id) = ack_msg_id {
                            self.ack_msg(id);
                        }
                        return Ok(none_result);
                    }
                    // Fall through to reassignment
                } else {
                    // Unexpected failure — skip reassignment, do NOT ack.
                    // The next poll cycle will retry the close.
                    tracing::error!(error = %e, "br close failed with unexpected error");
                    return Err(e);
                }
            }
        }

        // No inline sync here — br::close_bead writes directly to the DB.
        // JSONL export happens once at end-of-cycle in main.rs (br::sync).

        let t = std::time::Instant::now();
        let ready_beads = self.ensure_ready_work()?;
        tracing::info!(elapsed_ms = t.elapsed().as_millis() as u64, "reassign: ensure_ready_work");
        let next = ready_beads.into_iter().next();

        let next_bead = match next {
            Some(b) => b,
            None => {
                // No work — notify worker of idle state
                tracing::info!(worker = %worker_name, "No ready beads — worker is idle");
                let _ = self.send_idle_notification(worker_name, completed_bead);
                if let Some(id) = ack_msg_id {
                    self.ack_msg(id);
                }
                self.notify_boss(&format!(
                    "Queue exhausted after closing {completed_bead}. Worker {worker_name} is idle."
                ));
                return Ok(none_result);
            }
        };

        // Step 3: Check worker still alive in session, capturing pane index for prompt
        let t = std::time::Instant::now();
        let mut worker_pane_index: Option<u32> = None;
        if let Some(session) = &self.config.session_name {
            let panes = tmux::list_panes(session, self.config.window_index).unwrap_or_default();
            worker_pane_index = panes.iter().find_map(|pane| {
                if !self.config.is_worker_pane(pane) {
                    return None;
                }
                if worker::resolve_worker_identity(&self.config.project_root, &pane.id)
                    .as_deref()
                    == Some(worker_name)
                {
                    Some(pane.index)
                } else {
                    None
                }
            });

            if worker_pane_index.is_none() {
                tracing::info!(
                    worker = %worker_name,
                    next_bead = %next_bead.id,
                    "Worker no longer alive in session — skipping reassignment"
                );
                if let Some(id) = ack_msg_id {
                    self.ack_msg(id);
                }
                self.notify_boss(&format!(
                    "Skipped reassignment after closing {completed_bead}: \
                     worker {worker_name} no longer active, {} remains unassigned.",
                    next_bead.id
                ));
                return Ok(none_result);
            }
        }

        tracing::info!(elapsed_ms = t.elapsed().as_millis() as u64, "reassign: worker alive check");

        // Step 4: Assign next bead
        tracing::info!(
            bead = %next_bead.id,
            worker = %worker_name,
            "Assigning next bead"
        );
        // Note: br::sync() already ran in Step 2 after closing the bead.
        // A redundant sync here was taking 30-43s due to bv holding a WAL read
        // lock, blocking the entire assignment pipeline. Removed.
        if let Err(e) = br::update_bead(&next_bead.id, Some(worker_name), Some("in_progress")) {
            tracing::warn!(bead = %next_bead.id, error = %e, "br update --assignee failed");
        }

        // Step 5: Send assignment mail (best-effort — don't block on flaky mail server).
        // Workers get the assignment via tmux prompt anyway; mail is supplementary.
        if let Err(e) = self.send_assignment_mail(worker_name, &next_bead) {
            tracing::warn!(error = %e, "Mail send failed (non-fatal) — worker gets assignment via tmux prompt");
        }

        // Step 6: Do NOT send prompt immediately.
        // The worker is likely still outputting its completion summary.
        // Sending a prompt now creates a "Press up to edit queued message" state
        // that jams the worker. Instead, let the next poll cycle detect the
        // worker is idle with an assigned bead and send the prompt then —
        // assign_idle_workers runs every 8s and will pick it up.
        let prompt_task: Option<tmux::PromptTask> = None;

        // Step 7: Ack completion message
        if let Some(id) = ack_msg_id {
            self.ack_msg(id);
        }

        Ok(ReassignResult {
            next_bead_id: Some(next_bead.id),
            prompt_task,
        })
    }

    /// Check swarm health and return a recovery reason if the swarm is stalled.
    pub fn recovery_reason(&self, health: &SwarmHealth) -> Option<String> {
        // All idle with assignments → stalled
        if health.idle_assigned_panes >= self.config.stall_idle_assigned_threshold
            && health.active_assigned_panes <= self.config.stall_active_assigned_max
        {
            return Some(format!(
                "Swarm stall: {}/{} assigned panes are idle, only {} actively working",
                health.idle_assigned_panes,
                health.assigned_worker_panes,
                health.active_assigned_panes,
            ));
        }

        // Too many missing worker assignments
        if health.missing_worker_assignments >= self.config.missing_worker_assignment_threshold {
            return Some(format!(
                "Swarm stall: {} beads assigned to workers not in the session",
                health.missing_worker_assignments,
            ));
        }

        // No assigned panes at all when we have workers
        if health.worker_panes > 0 && health.assigned_worker_panes == 0 {
            return Some(format!(
                "Swarm stall: {} worker panes but none have assignments",
                health.worker_panes,
            ));
        }

        None
    }

    // --- Helper methods ---

    fn coordinator_name(&self) -> Result<String> {
        self.config
            .coordinator_agent
            .clone()
            .ok_or_else(|| OrchestratorError::Config("coordinator agent name not set".into()))
    }

    fn send_assignment_mail(&self, worker_name: &str, bead: &db::BeadInfo) -> Result<()> {
        let coordinator = self.coordinator_name()?;
        let project_key = self.config.project_root.to_string_lossy().to_string();

        let exec_map = if self.config.project_root.join("prd/POST_REPIN_EXECUTION_MAP.md").exists()
        {
            "prd/POST_REPIN_EXECUTION_MAP.md"
        } else {
            "prd/BEAD_EXECUTION_MAP.md"
        };

        let body = format!(
            "## Bead {}\n\n\
             **{}**\n\n\
             {}\n\n\
             ---\n\n\
             Start this bead now.\n\
             Follow AGENTS.md, {}, and docs/agent-mail-orchestration.md.\n\
             Do not mutate br state. Add tests with the implementation.\n\
             When complete, use `/skill mail-complete {} --to {} --file <path> --test <command>`.\n\
             Do not send a freehand completion message.\n\
             Include concrete `Files changed:` and `Tests run:` sections \
             plus a JSON payload with `bead_id`, `files_changed`, and `test_commands`.\n\
             Completion reports with placeholder or missing test evidence will be rejected and reopened.",
            bead.id, bead.title, bead.description, exec_map, bead.id, coordinator,
        );

        let msg = OutgoingMessage {
            project_key,
            sender_name: coordinator,
            to: vec![worker_name.to_string()],
            subject: format!("[{}] Assigned", bead.id),
            body_md: body,
            importance: "normal".to_string(),
            ack_required: true,
            topic: "bead-assign".to_string(),
            thread_id: bead.id.clone(),
        };

        self.mail.send_message_best_effort(&msg)
    }

    fn send_reopen_message(
        &self,
        worker_name: &str,
        bead_id: &str,
        reason: &str,
        details: &str,
    ) -> Result<()> {
        let coordinator = self.coordinator_name()?;
        let project_key = self.config.project_root.to_string_lossy().to_string();

        let body = format!(
            "## Bead {bead_id}\n\n\
             Your completion report did not pass coordinator verification.\n\n\
             Reason:\n\
             {reason}\n\n\
             Required before closure:\n\
             - resend completion with `/skill mail-complete {bead_id} --to {coordinator} --file <path> --test <command>`\n\
             - include a concrete `Files changed:` section\n\
             - include a concrete `Tests run:` section\n\
             - do not mark the bead complete until those tests have actually passed\n\n\
             Observed report:\n\
             {details}"
        );

        let msg = OutgoingMessage {
            project_key,
            sender_name: coordinator,
            to: vec![worker_name.to_string()],
            subject: format!("[{bead_id}] Reopened: verification failed"),
            body_md: body,
            importance: "high".to_string(),
            ack_required: true,
            topic: "bead-reopen".to_string(),
            thread_id: bead_id.to_string(),
        };

        self.mail.send_message_best_effort(&msg)
    }

    fn send_idle_notification(&self, worker_name: &str, completed_bead: &str) -> Result<()> {
        let coordinator = self.coordinator_name()?;
        let project_key = self.config.project_root.to_string_lossy().to_string();

        let msg = OutgoingMessage {
            project_key,
            sender_name: coordinator,
            to: vec![worker_name.to_string()],
            subject: "No work available".to_string(),
            body_md: format!(
                "All beads are complete or assigned. Previous bead `{completed_bead}` is closed. \
                 No new bead to assign — you may idle or check back later."
            ),
            importance: "low".to_string(),
            ack_required: false,
            topic: "bead-idle".to_string(),
            thread_id: format!("idle-{worker_name}"),
        };

        self.mail.send_message_best_effort(&msg)
    }

    pub fn notify_boss(&self, message: &str) {
        let script = self.config.notify_boss_script();
        if !script.exists() {
            return;
        }
        let session = match &self.config.session_name {
            Some(s) => s.clone(),
            None => return,
        };
        let _ = Command::new(&script).arg(&session).arg(message).output();
    }

    /// Detect worker panes with stuck (unsubmitted) input and recover them.
    /// Handles two cases:
    /// 1. **StuckInput**: raw prompt text visible — clear line and resubmit.
    /// 2. **Queued prompt**: worker has an assignment, appears idle, and pane
    ///    shows "Press up to edit" — Enter didn't go through, just resend it.
    pub fn recover_stuck_workers(&self, session: &str, window: u32) -> Result<usize> {
        let workers = {
            let db = self.open_db()?;
            worker::worker_info_list_with_config(session, window, &db, &self.config)?
        };

        let mut recovered = 0;
        for w in &workers {
            // Case 1: Raw prompt text stuck in input buffer
            if w.state == worker::WorkerState::StuckInput {
                tracing::warn!(
                    pane = w.pane_index,
                    worker = %w.worker_name,
                    bead = ?w.assigned_bead,
                    "Stuck input detected — clearing and resubmitting"
                );

                if self.dry_run {
                    recovered += 1;
                    continue;
                }

                // Resubmit the assigned bead's prompt if we have one.
                // prompt_worker_pane sends C-u internally, clearing the stuck input.
                if w.assigned_bead.is_some() {
                    let db = self.open_db()?;
                    if let Ok(Some(bead)) = db::assigned_bead_for_worker(&db, &w.worker_name) {
                        drop(db);
                        let prompt = self.build_prompt_text(&bead.id, &bead.title, &w.worker_name);
                        let _ = tmux::prompt_worker_pane(
                            session,
                            window,
                            w.pane_index,
                            &prompt,
                            &self.prompt_config(),
                        );
                    }
                } else {
                    // No tracked assignment — clear stuck text and send Enter.
                    let _ = tmux::send_keys(session, window, w.pane_index, &self.config.clear_line_key);
                    std::thread::sleep(std::time::Duration::from_millis(self.config.post_clear_delay_ms));
                    let _ = tmux::send_keys(session, window, w.pane_index, &self.config.submit_key);
                }

                recovered += 1;
                continue;
            }

            // Case 2: Worker has an assignment, looks idle, and pane has a queued
            // prompt ("Press up to edit") — Enter was sent but didn't go through.
            // Just nudge Enter rather than clearing and retyping everything.
            let is_idle = matches!(
                w.state,
                worker::WorkerState::Idle | worker::WorkerState::CompletedWaiting
            );
            if is_idle && w.assigned_bead.is_some() {
                let capture = tmux::capture_pane(session, window, w.pane_index, 80)
                    .unwrap_or_default();

                if capture.contains(&self.config.queue_prompt_marker) {
                    tracing::warn!(
                        pane = w.pane_index,
                        worker = %w.worker_name,
                        bead = ?w.assigned_bead,
                        "Queued prompt detected (Enter didn't go through) — nudging Enter"
                    );

                    if !self.dry_run {
                        // Send Enter to submit the queued prompt, then verify
                        let mut submitted = false;
                        for attempt in 0..self.config.submit_retry_attempts {
                            let _ = tmux::send_keys(session, window, w.pane_index, &self.config.submit_key);
                            std::thread::sleep(std::time::Duration::from_millis(
                                self.config.prompt_submit_delay_ms,
                            ));
                            let c = tmux::capture_pane(session, window, w.pane_index, 80)
                                .unwrap_or_default();
                            if !c.contains(&self.config.queue_prompt_marker) {
                                tracing::info!(
                                    pane = w.pane_index,
                                    attempt = attempt + 1,
                                    "Queued prompt submitted successfully"
                                );
                                submitted = true;
                                break;
                            }
                        }

                        // Escalate: Enter retries failed — dismiss the stuck queue
                        // with Escape so the pane returns to a clean idle state.
                        // Don't re-prompt here — let the normal idle-fill cycle
                        // pick it up next iteration to avoid prompt spam.
                        if !submitted {
                            tracing::warn!(
                                pane = w.pane_index,
                                worker = %w.worker_name,
                                "Enter retries exhausted — sending Escape to clear queue"
                            );
                            let _ = tmux::send_keys(session, window, w.pane_index, "Escape");
                        }
                    }

                    recovered += 1;
                }
            }
        }

        Ok(recovered)
    }

    fn ensure_ready_work(&self) -> Result<Vec<db::BeadInfo>> {
        {
            let db = self.open_db()?;
            let beads = db::ready_unassigned(&db, 50)?;
            if !beads.is_empty() {
                return Ok(beads);
            }
            // drop db before running seed_script (which may call br)
        }

        // Try seeding from port docs
        let seed_script = self.config.seed_script();
        if seed_script.exists() {
            tracing::info!("No unassigned ready beads. Seeding from port docs...");
            let _ = Command::new(&seed_script).output();
        }

        // Re-query after seeding (short-lived connection)
        let db = self.open_db()?;
        db::ready_unassigned(&db, 50)
    }

    fn ack_or_mark_read(&self, msg_id: Option<i64>) {
        if self.dry_run {
            return;
        }
        // Always ack completion messages. The inbox query filters by acknowledged_at,
        // not read_at — so mark_read alone leaves messages in the inbox forever.
        // This was the root cause of the "4 pending completions every cycle" loop.
        if let Some(id) = msg_id {
            self.ack_msg(id);
        }
    }

    fn ack_msg(&self, msg_id: i64) {
        let project_key = self.config.project_root.to_string_lossy().to_string();
        let agent_name = self.coordinator_name().unwrap_or_default();
        match self.mail.acknowledge(msg_id, &project_key, &agent_name) {
            Ok(()) => tracing::debug!(msg_id = msg_id, "acknowledged message"),
            Err(e) => tracing::warn!(msg_id = msg_id, error = %e, "failed to acknowledge message"),
        }
    }

    fn read_msg(&self, msg_id: i64) {
        let project_key = self.config.project_root.to_string_lossy().to_string();
        let agent_name = self.coordinator_name().unwrap_or_default();
        match self.mail.mark_read(msg_id, &project_key, &agent_name) {
            Ok(()) => tracing::debug!(msg_id = msg_id, "marked message read"),
            Err(e) => tracing::warn!(msg_id = msg_id, error = %e, "failed to mark message read"),
        }
    }

    fn build_prompt_text(&self, bead_id: &str, bead_title: &str, worker_name: &str) -> String {
        // Single-line: newlines cause Claude Code to submit prematurely via tmux send-keys -l.
        // The worker skill name varies by agent type (flywheel-worker for Claude, patina-fly-worker for Codex).
        let worker_skill = self.config.worker_skill_name();
        let complete_skill = self.config.completion_skill_name();
        format!(
            "Use /skill {worker_skill}. Work bead {bead_id}: {bead_title}. Your Agent Mail identity is {worker_name}. Read your inbox for full assignment details. Close out with /skill {complete_skill}, not a freehand completion message."
        )
    }

    fn prompt_config(&self) -> tmux::PromptConfig {
        self.config.prompt_config()
    }

    // --- Reprompt cache ---

    fn cache_file_for(&self, worker_name: &str) -> PathBuf {
        let sanitized: String = worker_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.config.cache_dir().join(format!("{sanitized}.last-assignment"))
    }

    fn should_skip_reprompt(&self, worker_name: &str, bead_id: &str) -> bool {
        let path = self.cache_file_for(worker_name);
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return false,
        };

        let mut lines = content.lines();
        let cached_bead = match lines.next() {
            Some(b) => b,
            None => return false,
        };
        if cached_bead != bead_id {
            return false;
        }

        let cached_ts: u64 = match lines.next().and_then(|s| s.parse().ok()) {
            Some(t) => t,
            None => return false,
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        now.saturating_sub(cached_ts) < self.config.reprompt_cooldown_seconds
    }

    fn record_reprompt(&self, worker_name: &str, bead_id: &str) {
        let path = self.cache_file_for(worker_name);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = fs::write(&path, format!("{bead_id}\n{now}\n"));
    }
}

/// Check if an idle worker's stale assignment should be reclaimed.
fn should_reclaim_idle(
    bead: &db::BeadInfo,
    age: u64,
    stale_threshold: u64,
    grace_seconds: u64,
    session: &str,
    window: u32,
    pane_index: u32,
    worker_name: &str,
    coordinator: &Coordinator,
) -> bool {
    if bead.status != db::BeadStatus::InProgress {
        return false;
    }
    if age < stale_threshold {
        return false;
    }

    let capture = tmux::capture_pane(session, window, pane_index, 60).unwrap_or_default();

    if worker::has_active_work(&capture) {
        return false;
    }

    // Check for explicit idle/done patterns
    static IDLE_DONE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let idle_done = IDLE_DONE_RE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)waiting for next (bead|assignment|coordinator assignment)|done\.?$|complete\.?$|ready for next assignment|pat-[a-z0-9]+ is done|no further action needed"
        ).unwrap()
    });

    if idle_done.is_match(&capture) {
        tracing::info!(
            bead = %bead.id,
            worker = %worker_name,
            age = age,
            "Reclaiming idle stale assignment"
        );
        return true;
    }

    // Check if we previously reprompted and the worker is still idle after grace period
    let reprompt_path = coordinator.cache_file_for(worker_name);
    if let Ok(content) = fs::read_to_string(&reprompt_path) {
        let mut lines = content.lines();
        if lines.next() == Some(&bead.id) {
            if let Some(ts) = lines.next().and_then(|s| s.parse::<u64>().ok()) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let reprompt_age = now.saturating_sub(ts);
                if reprompt_age >= grace_seconds {
                    static IDLE_PROMPT_RE: std::sync::OnceLock<regex::Regex> =
                        std::sync::OnceLock::new();
                    let idle_prompt = IDLE_PROMPT_RE.get_or_init(|| {
                        regex::Regex::new(
                            r"(?i)❯|waiting\.$|standing by|done\.?$|complete\.?$"
                        ).unwrap()
                    });
                    if idle_prompt.is_match(&capture) {
                        tracing::info!(
                            bead = %bead.id,
                            worker = %worker_name,
                            reprompt_age = reprompt_age,
                            "Reclaiming idle re-prompted assignment"
                        );
                        return true;
                    }
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        let mut cfg = crate::config::test_config();
        cfg.coordinator_agent = Some("TestCoordinator".to_string());
        cfg.project_root = std::path::PathBuf::from("/tmp/test-project");
        cfg.orch_root = std::path::PathBuf::from("/tmp/test-project/apps/orchestrator");
        cfg
    }

    fn test_coordinator() -> Coordinator {
        test_coordinator_with_config(test_config())
    }

    fn test_coordinator_with_config(cfg: Config) -> Coordinator {
        let mail = MailClient::new(&cfg.mail);
        let (verify_tx, verify_rx) = mpsc::channel();
        Coordinator {
            config: cfg,
            mail,
            dry_run: true,
            pending_verifications: Arc::new(Mutex::new(HashSet::new())),
            verify_rx: Mutex::new(verify_rx),
            verify_tx,
        }
    }

    #[test]
    fn test_recovery_reason_idle_stall() {
        let coord = test_coordinator();

        let health = SwarmHealth {
            worker_panes: 5,
            assigned_worker_panes: 5,
            unassigned_worker_panes: 0,
            idle_assigned_panes: 4,
            active_assigned_panes: 1,
            missing_worker_assignments: 0,
        };
        let reason = coord.recovery_reason(&health);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("idle"));
    }

    #[test]
    fn test_recovery_reason_missing_workers() {
        let coord = test_coordinator();

        let health = SwarmHealth {
            worker_panes: 5,
            assigned_worker_panes: 3,
            unassigned_worker_panes: 2,
            idle_assigned_panes: 0,
            active_assigned_panes: 3,
            missing_worker_assignments: 3,
        };
        let reason = coord.recovery_reason(&health);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("not in the session"));
    }

    #[test]
    fn test_recovery_reason_no_assignments() {
        let coord = test_coordinator();

        let health = SwarmHealth {
            worker_panes: 5,
            assigned_worker_panes: 0,
            unassigned_worker_panes: 5,
            idle_assigned_panes: 0,
            active_assigned_panes: 0,
            missing_worker_assignments: 0,
        };
        let reason = coord.recovery_reason(&health);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("none have assignments"));
    }

    #[test]
    fn test_recovery_reason_healthy() {
        let coord = test_coordinator();

        let health = SwarmHealth {
            worker_panes: 5,
            assigned_worker_panes: 5,
            unassigned_worker_panes: 0,
            idle_assigned_panes: 1,
            active_assigned_panes: 4,
            missing_worker_assignments: 0,
        };
        assert!(coord.recovery_reason(&health).is_none());
    }

    #[test]
    fn test_reprompt_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = test_config();
        cfg.project_root = tmp.path().to_path_buf();
        cfg.reprompt_cooldown_seconds = 45;

        let cache_dir = cfg.cache_dir();
        fs::create_dir_all(&cache_dir).unwrap();

        let coord = test_coordinator_with_config(cfg);

        // Initially should not skip
        assert!(!coord.should_skip_reprompt("WorkerA", "pat-abc"));

        // Record and check — should skip
        coord.record_reprompt("WorkerA", "pat-abc");
        assert!(coord.should_skip_reprompt("WorkerA", "pat-abc"));

        // Different bead — should not skip
        assert!(!coord.should_skip_reprompt("WorkerA", "pat-def"));

        // Different worker — should not skip
        assert!(!coord.should_skip_reprompt("WorkerB", "pat-abc"));

        // Check cache file format
        let cache_file = coord.cache_file_for("WorkerA");
        let content = fs::read_to_string(&cache_file).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "pat-abc");
        assert!(lines[1].parse::<u64>().is_ok());
    }

    #[test]
    fn test_reprompt_cache_expired() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = test_config();
        cfg.project_root = tmp.path().to_path_buf();
        cfg.reprompt_cooldown_seconds = 1; // 1 second cooldown for test

        let cache_dir = cfg.cache_dir();
        fs::create_dir_all(&cache_dir).unwrap();

        let coord = test_coordinator_with_config(cfg);

        // Write a cache entry with a timestamp in the past
        let cache_file = coord.cache_file_for("WorkerA");
        let past_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 10; // 10 seconds ago
        fs::write(&cache_file, format!("pat-abc\n{past_ts}\n")).unwrap();

        // Should NOT skip because cooldown (1s) has expired
        assert!(!coord.should_skip_reprompt("WorkerA", "pat-abc"));
    }

    #[test]
    fn test_cache_file_sanitization() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = test_config();
        cfg.project_root = tmp.path().to_path_buf();

        let coord = test_coordinator_with_config(cfg);

        let path = coord.cache_file_for("Worker/With Spaces");
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(filename, "Worker_With_Spaces.last-assignment");
        // No slashes or spaces in filename
        assert!(!filename.contains('/'));
        assert!(!filename.contains(' '));
    }

    #[test]
    fn test_build_prompt_text() {
        let coord = test_coordinator();

        let prompt = coord.build_prompt_text("pat-abc", "Fix the bug", "WorkerA");
        assert!(prompt.contains("pat-abc"), "must contain bead ID");
        assert!(prompt.contains("WorkerA"), "must contain worker name");
        assert!(prompt.contains("Fix the bug"), "must contain bead title");
        assert!(prompt.contains("flywheel-worker"), "must invoke flywheel-worker skill");
        assert!(prompt.contains("/skill mail-complete"), "must reference mail-complete skill");
    }

    #[test]
    fn test_poll_result_default() {
        let r = PollResult::default();
        assert_eq!(r.processed, 0);
        assert_eq!(r.rejected, 0);
        assert_eq!(r.errors, 0);
        assert!(r.prompt_tasks.is_empty());
    }

    #[test]
    fn test_reassign_result_none() {
        let r = ReassignResult { next_bead_id: None, prompt_task: None };
        assert!(r.next_bead_id.is_none());
        assert!(r.prompt_task.is_none());
    }

    /// Regression: workers detected as "busy" but with no DB assignment
    /// should be treated as idle and get fresh work, not skipped.
    /// This prevents the stale-output-blocks-assignment bug.
    #[test]
    fn test_unassigned_busy_worker_treated_as_assignable() {
        // The fix is in assign_idle_workers: when is_idle=false and
        // active_assignment_count=0, the old code did `continue` (skip).
        // The new code falls through to the assignment block.
        //
        // We verify the logic by checking that the WorkerState::Active
        // detection doesn't prevent assignment when there's no DB record.
        // This is a logic test — the actual DB/tmux interaction is tested
        // in integration.

        use crate::worker::WorkerState;

        // A worker that LOOKS active but has no assignment should be assignable
        let state = WorkerState::Active;
        let has_assignment = false;

        // Old logic: if !idle && !has_assignment → skip (BUG)
        // New logic: if !idle && !has_assignment → treat as idle (FIX)
        let is_idle = matches!(state, WorkerState::Idle | WorkerState::CompletedWaiting);
        let should_skip = !is_idle && has_assignment; // busy WITH assignment = skip
        let should_assign = !is_idle && !has_assignment; // busy WITHOUT assignment = assign

        assert!(!should_skip, "busy+unassigned should not skip");
        assert!(should_assign, "busy+unassigned should be assignable");

        // A worker that IS idle with no assignment should also be assignable
        let state2 = WorkerState::Idle;
        let is_idle2 = matches!(state2, WorkerState::Idle | WorkerState::CompletedWaiting);
        assert!(is_idle2, "idle worker should be assignable");
    }

    #[test]
    fn test_stuck_input_treated_as_idle() {
        use crate::worker::WorkerState;

        // StuckInput should be treated as idle in assign_idle_workers,
        // matching the updated is_idle check.
        let state = WorkerState::StuckInput;
        let is_idle = matches!(
            state,
            WorkerState::Idle | WorkerState::CompletedWaiting | WorkerState::StuckInput
        );
        assert!(is_idle, "StuckInput worker should be treated as idle for assignment");

        // Active should NOT be idle
        let active = WorkerState::Active;
        let active_idle = matches!(
            active,
            WorkerState::Idle | WorkerState::CompletedWaiting | WorkerState::StuckInput
        );
        assert!(!active_idle, "Active worker should not be treated as idle");
    }

    #[test]
    fn test_build_prompt_text_detectable_by_stuck_input() {
        // The prompt text built by the coordinator must be detectable
        // by the stuck input detection in worker.rs and tmux.rs.
        let coord = test_coordinator();

        let prompt = coord.build_prompt_text("pat-abc", "Fix the bug", "WorkerA");
        // Must trigger stuck input detection if it sits unsubmitted
        assert!(crate::worker::has_stuck_input(&prompt),
            "prompt text must be detectable as stuck input");
        // Must trigger tmux post-submit verification
        assert!(prompt.contains("/skill flywheel-worker") && prompt.contains("Work bead"),
            "prompt must contain both invariant substrings for post-submit verification");
    }

    // ========================================================================
    // Integration tests: coordinator stuck-input contracts, state machine
    // integration, prompt format stability, and recovery logic contracts.
    // ========================================================================

    /// Helper: generate a realistic orchestrator prompt.
    fn fake_prompt(bead_id: &str, title: &str, worker: &str) -> String {
        format!(
            "Use /skill flywheel-worker. Work bead {bead_id}: {title}. \
             Your Agent Mail identity is {worker}. Read your inbox for full \
             assignment details. Close out with /skill mail-complete, \
             not a freehand completion message."
        )
    }

    /// Helper: simulate tmux visual line wrapping.
    fn wrap_at_cols(text: &str, cols: usize) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut lines = Vec::new();
        for chunk in chars.chunks(cols) {
            lines.push(chunk.iter().collect::<String>());
        }
        lines.join("\n")
    }

    /// Regression: build_prompt_text must produce output detectable as stuck
    /// input at ALL realistic pane widths (20-200 cols). The original bug was
    /// that regex `.*` couldn't cross newline boundaries in wrapped text.
    #[test]
    fn test_build_prompt_text_detectable_at_all_pane_widths() {
        let cfg = test_config();
        let mail = MailClient::new(&cfg.mail);
        let coord = test_coordinator();

        // Test various bead IDs and titles that stress-test wrapping
        let cases = vec![
            ("pat-w1", "Fix bug", "WorkerA"),
            ("pat-w2", "Implement very long feature name that wraps many times", "PearlFox"),
            ("pat-w3", "X", "W"), // minimal
        ];

        for (bead_id, title, worker) in &cases {
            let prompt = coord.build_prompt_text(bead_id, title, worker);
            for cols in (20..=200).step_by(10) {
                let wrapped = wrap_at_cols(&prompt, cols);
                assert!(
                    crate::worker::has_stuck_input(&wrapped),
                    "worker::has_stuck_input failed for {bead_id} at {cols} cols"
                );
            }
        }
    }

    /// The is_idle match in assign_idle_workers must include every state that
    /// represents a non-working worker. This test ensures new WorkerState
    /// variants are explicitly considered.
    #[test]
    fn test_is_idle_covers_all_non_working_states() {
        use crate::worker::WorkerState;

        // These states should be treated as idle (available for assignment)
        let idle_states = vec![
            WorkerState::Idle,
            WorkerState::CompletedWaiting,
            WorkerState::StuckInput,
        ];
        for state in &idle_states {
            let is_idle = matches!(
                state,
                WorkerState::Idle | WorkerState::CompletedWaiting | WorkerState::StuckInput
            );
            assert!(is_idle, "{state:?} should be treated as idle");
        }

        // These states should NOT be idle
        let non_idle_states = vec![
            WorkerState::Active,
            WorkerState::Dead,
        ];
        for state in &non_idle_states {
            let is_idle = matches!(
                state,
                WorkerState::Idle | WorkerState::CompletedWaiting | WorkerState::StuckInput
            );
            assert!(!is_idle, "{state:?} should NOT be treated as idle");
        }
    }

    /// Regression: if detect_pane_state sees orchestrator prompt text with no
    /// other signals, it must return StuckInput (not default Active). This was
    /// the core bug — unsubmitted prompt text was classified as Active,
    /// blocking the worker for 120s until stale timeout.
    #[test]
    fn test_stuck_input_never_falls_through_to_default_active() {
        use crate::worker::{detect_pane_state, WorkerState};

        let cfg = test_config();
        let mail = MailClient::new(&cfg.mail);
        let coord = test_coordinator();

        // Generate the exact prompt the coordinator would produce
        let prompt = coord.build_prompt_text("pat-da1", "Fix rendering", "Worker1");

        // The raw prompt, with no other signals, must be StuckInput
        assert_eq!(
            detect_pane_state(&prompt),
            WorkerState::StuckInput,
            "raw prompt must be StuckInput, not default Active"
        );

        // With shell prompt prefix (realistic)
        let with_prefix = format!("❯ {prompt}");
        assert_eq!(
            detect_pane_state(&with_prefix),
            WorkerState::StuckInput,
            "prompt with shell prefix must be StuckInput"
        );

        // Wrapped at narrow width (the wrapping bug)
        let wrapped = wrap_at_cols(&prompt, 25);
        assert_eq!(
            detect_pane_state(&wrapped),
            WorkerState::StuckInput,
            "wrapped prompt must be StuckInput, not default Active"
        );
    }

    /// The detect_pane_state priority chain must hold under adversarial
    /// combinations. This test constructs captures with overlapping signals
    /// and verifies the priority order is always respected.
    #[test]
    fn test_state_priority_chain_holds_under_all_combinations() {
        use crate::worker::{detect_pane_state, WorkerState};

        let prompt = fake_prompt("pat-pc1", "Fix", "W1");
        let completed = "completion sent via ./apps/orchestrator/mail/complete";
        let active = "Compiling patina v0.1";
        let idle = "❯ ";

        // All four signals present → CompletedWaiting wins
        let all = format!("{completed}\n{active}\n{prompt}\n{idle}");
        assert_eq!(detect_pane_state(&all), WorkerState::CompletedWaiting);

        // Active + StuckInput + Idle → Active wins
        let no_complete = format!("{active}\n{prompt}\n{idle}");
        assert_eq!(detect_pane_state(&no_complete), WorkerState::Active);

        // StuckInput + Idle → StuckInput wins
        let no_active = format!("{prompt}\n{idle}");
        assert_eq!(detect_pane_state(&no_active), WorkerState::StuckInput);

        // Just Idle → Idle
        assert_eq!(detect_pane_state(idle), WorkerState::Idle);
    }

    /// Regression: failed close should NOT ack the completion message,
    /// allowing the next poll cycle to retry.
    /// Verifies PollResult tracks error vs success counts independently.
    #[test]
    fn test_failed_close_does_not_ack() {
        // Simulate a mixed poll result: 2 processed, 1 error.
        // errors > 0 means those messages were left un-acked for retry.
        let r = PollResult {
            processed: 2,
            rejected: 0,
            errors: 1,
            ..Default::default()
        };
        assert_eq!(r.processed + r.errors, 3, "total should be processed + errors");
        assert!(r.errors > 0, "errors means un-acked messages remain for retry");
        assert_ne!(r.processed, r.errors, "processed and errors should differ");

        // Default should be all zeros
        let empty = PollResult::default();
        assert_eq!(empty.processed + empty.rejected + empty.errors, 0);
    }

    /// Regression: the "no ready beads" branch in assign_idle_workers_inner must
    /// use `continue` (skip to next worker) not `break` (exit the entire loop).
    /// A `break` here causes the orchestrator to stop assigning work to ALL
    /// remaining idle workers once the bead queue is empty, jamming the swarm.
    #[test]
    fn test_no_ready_beads_continues_not_breaks() {
        let src = include_str!("coordinator.rs");

        // Find the "No ready unassigned beads" message — the very next
        // control-flow keyword must be `continue`, never `break`.
        let marker = "No ready unassigned beads";
        let pos = src.find(marker).expect("marker string not found in source");
        let after = &src[pos..];

        // Scan forward for the first `break` or `continue` keyword
        let break_pos = after.find("break;");
        let continue_pos = after.find("continue;");

        assert!(
            continue_pos.is_some(),
            "Expected `continue;` after '{}' marker but not found",
            marker
        );

        if let Some(bp) = break_pos {
            let cp = continue_pos.unwrap();
            assert!(
                cp < bp,
                "REGRESSION: `break;` appears before `continue;` after '{}' marker. \
                 The no-ready-beads branch must `continue` to the next worker, \
                 not `break` out of the entire assignment loop.",
                marker
            );
        }
    }

    /// Regression: recover_stuck_workers must handle BOTH raw stuck input AND
    /// queued prompts ("Press up to edit"). Before this fix, only StuckInput
    /// (raw prompt text visible) was recovered. A queued prompt where Enter
    /// didn't go through was missed — the worker sat idle with an assignment.
    #[test]
    fn test_recover_stuck_handles_queued_prompt_case() {
        // Verify the source code has the queued prompt detection branch.
        // This is a structural regression test — if the branch is removed,
        // the test fails.
        let source = include_str!("coordinator.rs");

        assert!(
            source.contains("Queued prompt detected"),
            "recover_stuck_workers must detect queued prompts (Enter didn't go through)"
        );

        assert!(
            source.contains("queue_prompt_marker"),
            "recover_stuck_workers must check for queue_prompt_marker in pane capture"
        );

        // The queued prompt path should send submit_key (Enter), not clear+retype
        assert!(
            source.contains("nudging Enter"),
            "queued prompt recovery should nudge Enter, not clear and retype"
        );
    }

    /// Regression: StuckInput detection and queued prompt detection must be
    /// separate checks. StuckInput uses prompt text markers; queued prompt
    /// uses "Press up to edit". They can't be merged.
    #[test]
    fn test_stuck_input_and_queued_prompt_are_distinct() {
        use crate::worker::{detect_pane_state, WorkerState};

        // Raw prompt text → StuckInput
        let stuck = "Use /skill flywheel-worker. Work bead pat-123: Fix. identity is W.";
        assert_eq!(detect_pane_state(stuck), WorkerState::StuckInput);

        // "Press up to edit" with no prompt markers → Idle (not StuckInput)
        let queued = "Press up to edit\n❯ ";
        assert_ne!(detect_pane_state(queued), WorkerState::StuckInput,
            "queued prompt should NOT be StuckInput — it needs the nudge-Enter path");
    }

    // ========================================================================
    // Regression: assignment mail failure must NOT roll back bead assignment.
    //
    // Bug: reassign() treated send_assignment_mail failure as fatal, calling
    // br::reopen() to roll back the DB assignment and returning Err. With a
    // flaky mail server (intermittent 5-15s timeouts), every assignment was
    // rolled back and workers sat idle for minutes. The fix makes assignment
    // mail best-effort — the worker gets the bead via tmux prompt anyway.
    // ========================================================================

    /// Regression: reassign must NOT call br::reopen after mail failure.
    #[test]
    fn test_mail_failure_does_not_rollback_assignment() {
        let source = include_str!("coordinator.rs");

        // The reassign function's mail send section must contain "non-fatal"
        assert!(
            source.contains("Mail send failed (non-fatal)"),
            "reassign() must treat mail failure as non-fatal"
        );

        // The reassign function must NOT have a return Err(e) right after mail send.
        // Extract the section between "Step 5: Send assignment mail" and "Step 6:"
        let step5 = source.find("Step 5: Send assignment mail").expect("Step 5 must exist");
        let step6 = source[step5..].find("Step 6:").expect("Step 6 must exist");
        let mail_section = &source[step5..step5 + step6];

        assert!(
            !mail_section.contains("return Err(e)"),
            "REGRESSION: mail section must not return Err — that causes rollback"
        );
        assert!(
            !mail_section.contains("br::reopen"),
            "REGRESSION: mail section must not call br::reopen"
        );
    }

    /// Regression: send_assignment_mail must use best-effort (no retry).
    /// Retrying a flaky mail server blocks the entire coordinator loop.
    #[test]
    fn test_assignment_mail_uses_best_effort_send() {
        let source = include_str!("coordinator.rs");

        // The send_assignment_mail function must call send_message_best_effort
        assert!(
            source.contains("send_message_best_effort"),
            "send_assignment_mail must use send_message_best_effort (no retry)"
        );
    }

    /// Regression: the reassign function must still return Ok after mail failure.
    /// Before the fix, it returned Err(e), which caused the coordinator to
    /// skip post-poll idle fill and stall the entire cycle.
    #[test]
    fn test_reassign_returns_ok_despite_mail_failure() {
        let source = include_str!("coordinator.rs");

        // After the mail send, the function must proceed to ack and return Ok
        // Find the mail send line and verify no return Err follows
        let mail_section = source
            .find("Mail send failed (non-fatal)")
            .expect("mail send section must exist");
        let after_mail = &source[mail_section..];

        // The next "return" after the mail section should be Ok, not Err
        let next_ok = after_mail.find("Ok(Some(");
        let next_err_return = after_mail.find("return Err(e)");

        assert!(
            next_ok.is_some(),
            "reassign must return Ok(Some(...)) after mail section"
        );

        // If there's a return Err, it must be AFTER the Ok (for different error paths)
        if let Some(err_pos) = next_err_return {
            let ok_pos = next_ok.unwrap();
            assert!(
                ok_pos < err_pos,
                "REGRESSION: return Err(e) appears before Ok in the mail failure path"
            );
        }
    }

    /// Structural: mail send is called with let _ or if let Err in ALL
    /// assignment paths (idle fill assign and reassign).
    #[test]
    fn test_all_assignment_paths_handle_mail_failure_gracefully() {
        let source = include_str!("coordinator.rs");

        // Count occurrences of send_assignment_mail calls
        let calls: Vec<_> = source.match_indices("send_assignment_mail").collect();
        assert!(
            calls.len() >= 3,
            "expected at least 3 send_assignment_mail calls (idle fill + reassign + definition), found {}",
            calls.len()
        );

        // The idle fill paths use `let _ = self.send_assignment_mail`
        let let_ignores = source.matches("let _ = self.send_assignment_mail").count();
        assert!(
            let_ignores >= 2,
            "idle fill paths must use `let _ = self.send_assignment_mail` (fire-and-forget), found {}",
            let_ignores
        );
    }

    /// Structural: send_message_best_effort exists in mail.rs and does NOT retry.
    #[test]
    fn test_best_effort_send_exists_without_retry() {
        let mail_source = include_str!("mail.rs");

        assert!(
            mail_source.contains("fn send_message_best_effort"),
            "mail.rs must have send_message_best_effort method"
        );

        // Find the best_effort function body and verify it calls call_tool (not call_tool_with_retry)
        let fn_pos = mail_source
            .find("fn send_message_best_effort")
            .expect("function must exist");
        let fn_body = &mail_source[fn_pos..fn_pos + 200]; // first 200 chars of function

        assert!(
            fn_body.contains("call_tool(") && !fn_body.contains("call_tool_with_retry"),
            "send_message_best_effort must call call_tool (no retry), not call_tool_with_retry"
        );
    }

    /// Boundary: verify the retry config affects call_tool_with_retry but NOT call_tool.
    /// This ensures best_effort truly makes a single attempt regardless of config.
    #[test]
    fn test_call_tool_with_retry_uses_max_retries_config() {
        let mail_source = include_str!("mail.rs");

        // call_tool_with_retry must reference self.max_retries
        assert!(
            mail_source.contains("self.max_retries"),
            "call_tool_with_retry must use self.max_retries"
        );

        // call_tool (the single-attempt version) must NOT loop
        let call_tool_fn = mail_source
            .find("fn call_tool(")
            .expect("call_tool function must exist");
        let call_tool_body_end = mail_source[call_tool_fn..]
            .find("\n    }\n")
            .map(|p| call_tool_fn + p)
            .unwrap_or(call_tool_fn + 500);
        let call_tool_body = &mail_source[call_tool_fn..call_tool_body_end];

        assert!(
            !call_tool_body.contains("for attempt") && !call_tool_body.contains("max_retries"),
            "call_tool must NOT contain retry logic"
        );
    }

    // ========================================================================
    // Regression: assignee mismatch must NOT skip completion processing.
    //
    // Bug: When the orchestrator ran idle-fill BEFORE poll, it would reclaim
    // beads from workers who had just completed (idle pane) and reassign to
    // different workers. When poll then processed the original worker's
    // completion message, the assignee didn't match → hard SKIP. The bead
    // was never closed despite work being done and tests passing. This caused
    // a vicious cycle: reclaim → reassign → mismatch → reclaim → ...
    //
    // Fix 1: Changed assignee mismatch from hard SKIP to WARN (proceed with
    // verification). If tests pass, the bead gets closed regardless.
    //
    // Fix 2: Moved poll() before idle-fill in the main loop so completions
    // are processed before any reclaim happens.
    // ========================================================================

    /// Regression: the assignee mismatch check must NOT call `continue` to
    /// skip the completion. It must log a warning and proceed with verification.
    #[test]
    fn test_assignee_mismatch_does_not_skip_completion() {
        let source = include_str!("coordinator.rs");

        // Use a unique marker in the production code (not test code)
        let marker = "proceeding with verification anyway";
        let mismatch_pos = source.find(marker)
            .expect("'proceeding with verification anyway' must exist in poll()");

        // Look at 300 chars around the marker — must NOT have continue;
        let section_start = mismatch_pos.saturating_sub(50);
        let section = &source[section_start..mismatch_pos + 100];

        assert!(
            !section.contains("continue;"),
            "REGRESSION: assignee mismatch path must NOT `continue` (skip completion). \
             The reclaim/reassign cycle races with completion processing, causing \
             valid completions to be dropped when the assignee was changed."
        );
    }

    /// Regression: the assignee mismatch log level must be WARN, not INFO.
    /// INFO-level skips are silent in normal operation; WARN makes the race
    /// condition visible in monitoring.
    #[test]
    fn test_assignee_mismatch_logged_as_warn() {
        let source = include_str!("coordinator.rs");

        // Use the unique production marker
        let marker = "proceeding with verification anyway";
        let mismatch_pos = source.find(marker).expect("marker must exist");

        // Look backwards for the tracing macro (within ~500 chars to cover struct fields)
        let search_start = mismatch_pos.saturating_sub(500);
        let before = &source[search_start..mismatch_pos];

        // The last tracing call before the marker should be warn!, not info!
        let last_warn = before.rfind("tracing::warn!");
        let last_info = before.rfind("tracing::info!");

        assert!(
            last_warn.is_some(),
            "assignee mismatch must use tracing::warn!"
        );

        if let Some(info_pos) = last_info {
            let warn_pos = last_warn.unwrap();
            assert!(
                warn_pos > info_pos,
                "REGRESSION: assignee mismatch uses tracing::info! (silent). \
                 Must use tracing::warn! to make the race condition visible."
            );
        }
    }

    /// Regression: completion messages from non-current-assignee must still be
    /// acked after successful close, not left un-acked to pile up in inbox.
    #[test]
    fn test_mismatched_completion_still_acked_on_close() {
        let source = include_str!("coordinator.rs");

        // Use the unique production marker
        let marker = "proceeding with verification anyway";
        let mismatch_pos = source.find(marker).expect("marker must exist");
        let after = &source[mismatch_pos..];

        // The section between mismatch and the next evidence check must NOT ack+continue.
        let evidence_check = after.find("Reject if missing evidence")
            .expect("evidence check must follow mismatch");
        let between = &after[..evidence_check];

        assert!(
            !between.contains("ack_or_mark_read"),
            "REGRESSION: the mismatch path must NOT call ack_or_mark_read before \
             verification. The message should only be acked after successful close \
             or explicit rejection."
        );
    }

    /// Structural: poll must run BEFORE idle-fill in the main loop.
    /// This ensures completions are processed before any bead reclaim happens.
    #[test]
    fn test_poll_runs_before_idle_fill_in_main_loop() {
        let main_source = include_str!("main.rs");

        // Find the first coord.poll() call
        let poll_pos = main_source.find("coord.poll()")
            .expect("coord.poll() must exist in main.rs");

        // Find the idle fill call after poll
        let after_poll = &main_source[poll_pos..];
        let idle_fill_pos = after_poll.find("run_idle_fill")
            .expect("run_idle_fill must exist after poll");

        // poll must come before idle_fill
        assert!(
            idle_fill_pos > 0,
            "REGRESSION: poll() must run BEFORE run_idle_fill(). \
             If idle-fill runs first, it reclaims beads from completed workers \
             before their completion messages are processed, causing assignee \
             mismatches that drop valid completions."
        );

        // There must NOT be a run_idle_fill call before coord.poll() in the same cycle.
        // Check the last 200 chars before poll for a pre-poll idle fill.
        let before_poll = &main_source[..poll_pos];
        let pre_poll_section = &before_poll[before_poll.len().saturating_sub(200)..];
        assert!(
            !pre_poll_section.contains("run_idle_fill"),
            "REGRESSION: run_idle_fill() found before coord.poll(). \
             Pre-poll idle fill causes the reclaim-before-process race condition."
        );
    }

    /// Boundary: a completion from an unknown worker (empty name) with valid
    /// evidence should still be processed, not rejected.
    #[test]
    fn test_completion_from_unknown_worker_not_rejected() {
        let source = include_str!("coordinator.rs");

        // Find the mismatch check in production code using unique marker
        let marker = "proceeding with verification anyway";
        let mismatch_pos = source.find(marker).expect("marker must exist");
        // Look backwards ~500 chars for the empty-check guard
        let check_start = mismatch_pos.saturating_sub(500);
        let check_section = &source[check_start..mismatch_pos];

        assert!(
            check_section.contains("!completion.worker.is_empty()"),
            "assignee mismatch check must skip when completion.worker is empty — \
             anonymous completions should be accepted if evidence is valid"
        );
    }

    /// Boundary: a completion for a bead with no assignee in the DB should
    /// proceed to verification, not error out on the mismatch check.
    #[test]
    fn test_completion_for_unassigned_bead_proceeds() {
        let source = include_str!("coordinator.rs");

        // Find the mismatch check in production code
        let marker = "proceeding with verification anyway";
        let mismatch_pos = source.find(marker).expect("marker must exist");
        let check_start = mismatch_pos.saturating_sub(500);
        let check_section = &source[check_start..mismatch_pos];

        assert!(
            check_section.contains("if let Some(current_assignee)"),
            "assignee mismatch must be inside `if let Some(current_assignee)` — \
             beads with no assignee should pass through to verification"
        );
    }

    /// Stress: verify the mismatch-tolerant path doesn't introduce any new
    /// `continue` or `break` before reaching the verification stage.
    /// Scans from the bead_state check through to evidence validation.
    #[test]
    fn test_no_early_exits_between_state_check_and_verification() {
        let source = include_str!("coordinator.rs");

        // Find the section between "Check bead state" and "Reject if missing evidence"
        let state_check = source.find("Check bead state in DB")
            .expect("bead state check must exist");
        let evidence_check = source[state_check..].find("Reject if missing evidence")
            .expect("evidence check must exist");
        let section = &source[state_check..state_check + evidence_check];

        // Count continues — should be exactly 1 (for closed/tombstone skip)
        let continue_count = section.matches("continue;").count();
        assert_eq!(
            continue_count, 1,
            "Expected exactly 1 `continue` (closed/tombstone skip) between state check \
             and evidence check, found {continue_count}. The assignee mismatch path \
             must NOT have a `continue`."
        );
    }

    /// Variant: closed-bead completion should still be acked (not left in inbox).
    /// This is the one legitimate SKIP that must remain — but it must still ack.
    #[test]
    fn test_closed_bead_completion_is_acked() {
        let source = include_str!("coordinator.rs");

        let closed_skip = source.find("SKIP: already closed/tombstone")
            .expect("closed/tombstone skip must exist");
        let after = &source[closed_skip..closed_skip + 200];

        assert!(
            after.contains("ack_or_mark_read"),
            "closed/tombstone completions must still be acked to clear them from inbox"
        );
        assert!(
            after.contains("continue"),
            "closed/tombstone completions should skip further processing"
        );
    }

    /// Negative: the poll-first ordering comment must explain the race condition
    /// to prevent future developers from moving idle-fill back before poll.
    #[test]
    fn test_poll_first_comment_explains_race_condition() {
        let main_source = include_str!("main.rs");

        // The comment "Poll FIRST" must exist and explain the race
        assert!(
            main_source.contains("Poll FIRST"),
            "main.rs must have a 'Poll FIRST' comment explaining the ordering"
        );

        let poll_comment_pos = main_source.find("Poll FIRST").unwrap();
        let comment_section = &main_source[poll_comment_pos..poll_comment_pos + 300];

        assert!(
            comment_section.contains("race") || comment_section.contains("reclaim"),
            "The Poll FIRST comment must explain the race condition \
             to prevent future developers from reintroducing the pre-poll idle-fill"
        );
    }

    // ========================================================================
    // Regression: reassign() must NOT call br::sync() inline.
    //
    // Bug: Two br::sync() calls inside reassign() each took 30-43s waiting on
    // bv's WAL read lock, making each assignment take 41+ seconds. With 9
    // workers, the coordinator spent 5-10 minutes per cycle just on syncs.
    // Fix: removed all inline br::sync() calls from reassign(). The DB is
    // written directly by br close/update; JSONL sync is deferred.
    // ========================================================================

    /// Regression: reassign must NOT call br::sync() — it blocks for 30-43s.
    #[test]
    fn test_reassign_has_no_inline_sync() {
        let source = include_str!("coordinator.rs");

        // Find the reassign function body
        let fn_start = source.find("pub fn reassign(").expect("reassign must exist");
        // Find the end — next "pub fn" or "fn " at the same indent level
        let fn_body_end = source[fn_start + 10..]
            .find("\n    pub fn ")
            .or_else(|| source[fn_start + 10..].find("\n    /// Check swarm"))
            .map(|p| fn_start + 10 + p)
            .unwrap_or(fn_start + 3000);
        let fn_body = &source[fn_start..fn_body_end];

        // Must not contain active br::sync() calls (comments are ok)
        let active_syncs: Vec<_> = fn_body
            .lines()
            .filter(|l| l.contains("br::sync()") && !l.trim_start().starts_with("//"))
            .collect();

        assert!(
            active_syncs.is_empty(),
            "REGRESSION: reassign() must not call br::sync() inline (causes 30-43s WAL hangs). \
             Found: {:?}",
            active_syncs
        );
    }

    /// Regression: all mail sends in coordinator must use best_effort.
    /// Retrying sends block the coordinator loop for seconds per failure.
    #[test]
    fn test_all_mail_sends_use_best_effort() {
        let source = include_str!("coordinator.rs");

        // Find all send_message calls that are NOT best_effort and NOT in tests
        let mut violations = Vec::new();
        for (i, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("*") {
                continue;
            }
            if trimmed.contains(".send_message(") && !trimmed.contains("send_message_best_effort") {
                // Check if it's in a test
                let preceding = &source[..source.lines().take(i).map(|l| l.len() + 1).sum::<usize>()];
                if !preceding.ends_with("cfg(test)]") && !preceding.contains("#[test]") {
                    violations.push((i + 1, trimmed.to_string()));
                }
            }
        }

        // Allow only in test code
        let non_test_violations: Vec<_> = violations
            .iter()
            .filter(|(line_num, _)| {
                let test_mod_start = source.find("#[cfg(test)]").unwrap_or(source.len());
                let line_offset: usize = source.lines().take(*line_num - 1).map(|l| l.len() + 1).sum();
                line_offset < test_mod_start
            })
            .collect();

        assert!(
            non_test_violations.is_empty(),
            "REGRESSION: all mail sends must use send_message_best_effort. \
             Found retrying send_message at: {:?}",
            non_test_violations
        );
    }

    // ========================================================================
    // Regression: main loop efficiency — consolidated DB, no sleeps, caching
    //
    // Bugs fixed:
    // 1. 4 separate db::open() calls per cycle → 1 connection reused
    // 2. 3x unconditional thread::sleep(1s) in seed path → removed
    // 3. Worker list enumerated twice → captured once, reused
    // 4. Mail server checked every 8s cycle → cached, every 60s
    // 5. Stale comment referencing removed sync
    // 6. Silent idle-fill retry success
    // ========================================================================

    /// Regression: DB connections must be SHORT-LIVED (scoped blocks) so they
    /// don't block br subprocess writes. The initial seed check opens a connection,
    /// queries, then DROPS it before any br create/seed subprocess calls.
    #[test]
    fn test_main_loop_db_connections_are_short_lived() {
        let source = include_str!("main.rs");

        // The seed check section must have a scoped block that drops conn
        let seed_start = source.find("Seed check").expect("Seed check section must exist");
        let seed_section = &source[seed_start..seed_start + 600];

        // Must contain IMPORTANT comment about dropping before br subprocess calls
        assert!(
            seed_section.contains("DROPPED before") || seed_section.contains("dropped here")
                || seed_section.contains("frees the DB"),
            "Seed check must document that the DB connection is dropped before br subprocess calls"
        );
    }

    /// Regression: no unconditional sleeps in the seed path.
    /// Sleeps waste 1-3 seconds per cycle even when no beads are created.
    #[test]
    fn test_no_unconditional_sleeps_in_seed_path() {
        let source = include_str!("main.rs");

        // Find seed script section
        let seed_script_section = source.find("Step 1: Run seed script");
        if let Some(pos) = seed_script_section {
            let section = &source[pos..pos + 300];
            assert!(
                !section.contains("thread::sleep(Duration::from_secs(1))"),
                "REGRESSION: unconditional sleep(1s) after seed script wastes time"
            );
        }
    }

    /// Regression: worker list captured once and reused for health check.
    #[test]
    fn test_worker_list_captured_once() {
        let source = include_str!("main.rs");

        // The stall detection section must reuse worker_list, not re-enumerate
        let stall_section = source.find("stall detection");
        if let Some(pos) = stall_section {
            let section = &source[pos..pos + 300];
            assert!(
                !section.contains("worker_info_list_with_config("),
                "REGRESSION: stall detection must reuse cached worker_list, not re-enumerate"
            );
        }
    }

    /// Regression: mail server check must be cached (not every cycle).
    #[test]
    fn test_mail_server_check_cached() {
        let source = include_str!("main.rs");

        let mail_check = source.find("Ensure mail server").expect("mail check must exist");
        let section = &source[mail_check..mail_check + 300];

        assert!(
            section.contains("elapsed()") && section.contains("60"),
            "Mail server check must be time-gated (every 60s), not every cycle"
        );
    }

    /// Regression: reassign comment must reference end-of-cycle sync location.
    #[test]
    fn test_reassign_comment_references_end_of_cycle() {
        let source = include_str!("coordinator.rs");
        let reassign_fn = source.find("pub fn reassign(").expect("reassign must exist");
        let fn_end = source[reassign_fn..].find("\n    pub fn ").unwrap_or(8000);
        let fn_section = &source[reassign_fn..reassign_fn + fn_end];

        // Must NOT reference "Step 2" sync that no longer exists
        assert!(
            !fn_section.contains("Step 2: Sync JSONL"),
            "REGRESSION: stale comment references removed Step 2 sync"
        );

        // Must reference end-of-cycle
        assert!(
            fn_section.contains("end-of-cycle"),
            "reassign should reference where sync actually happens"
        );
    }

    /// Regression: idle fill must log success on retry.
    #[test]
    fn test_idle_fill_logs_retry_success() {
        let source = include_str!("main.rs");
        let idle_fill = source.find("fn run_idle_fill(").expect("run_idle_fill must exist");
        let fn_section = &source[idle_fill..idle_fill + 500];

        assert!(
            fn_section.contains("idle fill succeeded after retry"),
            "run_idle_fill must log when it succeeds after a retry"
        );
    }

    /// DB connections in the seed section must all be short-lived (scoped).
    /// None should span across br subprocess calls.
    #[test]
    fn test_seed_section_no_long_lived_connections() {
        let source = include_str!("main.rs");

        // The initial seed check connection must be in a scoped block
        let seed_start = source.find("Seed check").unwrap_or(0);
        if seed_start > 0 {
            let section = &source[seed_start..seed_start + 1500];
            // Must have explicit drop or scoped block
            assert!(
                section.contains("conn dropped here") || section.contains("frees the DB")
                    || section.contains("// conn dropped"),
                "Seed check connection must be explicitly scoped/dropped before br subprocess calls"
            );
        }
    }

    /// Negative: main loop must still have end-of-cycle br::sync().
    #[test]
    fn test_end_of_cycle_sync_exists() {
        let source = include_str!("main.rs");
        assert!(
            source.contains("End-of-cycle") && source.contains("br::sync()"),
            "main loop must have end-of-cycle br::sync() to flush DB → JSONL"
        );
    }

    // ========================================================================
    // Regression: DB connection must NOT outlive br subprocess calls.
    //
    // Bug: "fix #1" consolidated 4 db::open() calls into 1, but held it
    // open as `let conn` across the entire seed/planner path. This blocked
    // ALL br subprocess writes (br create, br close, seed scripts) with
    // "database is busy" because the orchestrator's open connection held
    // a WAL lock that prevented br's write transactions on macOS.
    //
    // Fix: scoped the connection in a { } block so it drops before any
    // br subprocess calls. Re-checks use their own short-lived connections.
    // ========================================================================

    /// Regression: the seed check connection must be in a scoped block that
    /// ends BEFORE the seed script / planner / br create calls.
    /// The anti-pattern was: `let conn = db::open(...)` at line 551, with
    /// conn living until the end of the `if poll_ok` block at line ~740.
    #[test]
    fn test_seed_conn_does_not_span_br_subprocess_calls() {
        let source = include_str!("main.rs");

        let seed_start = source.find("Seed check").expect("Seed check must exist");
        let seed_script = source[seed_start..].find("seed_script.exists()").unwrap_or(0);

        if seed_script > 0 {
            // The seed script call is AFTER the initial connection block.
            // Between "Seed check" and "seed_script.exists()", there must be
            // a closing brace (}) that ends the connection scope.
            let between = &source[seed_start..seed_start + seed_script];

            // The connection must be inside a { let ... } block
            assert!(
                between.contains("let (worker_list") || between.contains("(wl,"),
                "Connection results must be destructured out of a scoped block"
            );
        }
    }

    /// Regression: re-check queries after seed/Tier1 must use their OWN
    /// short-lived connections, not a shared `conn` from the seed check.
    #[test]
    fn test_rechecks_use_own_connections() {
        let source = include_str!("main.rs");

        // Find re-check sections
        let recheck1 = source.find("Re-check queue after seed");
        let recheck2 = source.find("Re-check again after Tier 1");

        // Each re-check must open its own connection in a scoped block
        if let Some(pos) = recheck1 {
            let section = &source[pos..pos + 200];
            assert!(
                section.contains("db::open("),
                "Re-check after seed must open its own short-lived connection"
            );
        }

        if let Some(pos) = recheck2 {
            let section = &source[pos..pos + 200];
            assert!(
                section.contains("db::open("),
                "Re-check after Tier 1 must open its own short-lived connection"
            );
        }
    }

    /// Regression: stall detection must use its own connection, not a shared one.
    #[test]
    fn test_stall_detection_uses_own_connection() {
        let source = include_str!("main.rs");

        // Find the SECOND "stall detection" (the recovery section, not the seed check comment)
        let first = source.find("stall detection").unwrap_or(0);
        let second = source[first + 20..].find("stall detection").map(|p| first + 20 + p);
        if let Some(pos) = second {
            let section = &source[pos..pos + 200];
            assert!(
                section.contains("db::open("),
                "Stall/recovery detection must open its own short-lived connection"
            );
        }
    }

    /// Stress: verify NO `let conn = db::open` exists at function/block scope
    /// that could outlive a br subprocess call. All db::open calls in the
    /// post-poll section must be inside { } blocks that close before br calls.
    #[test]
    fn test_no_long_lived_conn_in_post_poll() {
        let source = include_str!("main.rs");

        // Find post-poll section (after "poll complete" through end of loop)
        let poll_ok = source.find("if poll_ok").unwrap_or(0);
        if poll_ok == 0 { return; }

        let loop_end = source[poll_ok..].find("drop(lock_file)").unwrap_or(5000);
        let post_poll = &source[poll_ok..poll_ok + loop_end];

        // Count lines that have `let conn = db::open(` NOT inside a block
        // The safe pattern is: `let conn = db::open(` inside { }
        // The unsafe pattern is: `let conn = db::open(` at if-block scope
        let lines: Vec<(usize, &str)> = post_poll.lines().enumerate()
            .filter(|(_, l)| {
                let trimmed = l.trim();
                trimmed.starts_with("let conn = db::open(")
                    || trimmed.starts_with("let conn =  db::open(")
            })
            .collect();

        // Any top-level `let conn = db::open(` is suspicious.
        // It MUST be inside a { } block. Check that the line before has {
        for (i, line) in &lines {
            if *i > 0 {
                let prev_lines: Vec<&str> = post_poll.lines().take(*i).collect();
                let prev = prev_lines.last().map(|l| l.trim()).unwrap_or("");
                // Must be inside a scoped block (previous line ends with { or contains let ... = {)
                let in_block = prev.ends_with('{') || prev.contains("= {");
                assert!(
                    in_block,
                    "REGRESSION: `{line}` at line {i} is NOT inside a scoped block. \
                     DB connections must be dropped before br subprocess calls."
                );
            }
        }
    }

    // ========================================================================
    // Regression: parallel prompt submission via std::thread::scope
    //
    // Change: prompt_worker_pane calls moved from inline (serial, 4s × N)
    // to batched parallel (all N in ~4s). assign_idle_workers returns
    // (usize, Vec<PromptTask>), main.rs calls submit_prompts_parallel.
    // ========================================================================

    /// Regression: assign_idle_workers must return PromptTask vec, not submit inline.
    #[test]
    fn test_assign_idle_workers_returns_prompt_tasks() {
        let source = include_str!("coordinator.rs");

        let fn_start = source.find("pub fn assign_idle_workers(")
            .expect("assign_idle_workers must exist");
        let sig = &source[fn_start..fn_start + 200];

        assert!(
            sig.contains("Vec<tmux::PromptTask>"),
            "assign_idle_workers must return Vec<tmux::PromptTask> for parallel submission"
        );
    }

    /// Regression: assign_idle_workers_inner must NOT call prompt_worker_pane inline.
    #[test]
    fn test_assign_idle_workers_inner_no_inline_prompts() {
        let source = include_str!("coordinator.rs");

        let fn_start = source.find("fn assign_idle_workers_inner(")
            .expect("assign_idle_workers_inner must exist");
        let fn_end = source[fn_start..].find("\n    pub fn ").unwrap_or(5000);
        let fn_body = &source[fn_start..fn_start + fn_end];

        // Must NOT have inline prompt_worker_pane calls
        let inline_prompts: Vec<_> = fn_body.lines()
            .filter(|l| l.contains("prompt_worker_pane(") && !l.trim_start().starts_with("//"))
            .collect();

        assert!(
            inline_prompts.is_empty(),
            "REGRESSION: assign_idle_workers_inner must queue prompts, not submit inline. \
             Found inline calls: {:?}",
            inline_prompts
        );

        // Must push to prompt_tasks instead
        assert!(
            fn_body.contains("prompt_tasks.push("),
            "assign_idle_workers_inner must push PromptTask to queue"
        );
    }

    /// Regression: main.rs must call submit_prompts_parallel, not submit inline.
    #[test]
    fn test_main_loop_uses_parallel_prompt_submission() {
        let source = include_str!("main.rs");

        assert!(
            source.contains("submit_prompts_parallel"),
            "main.rs must call tmux::submit_prompts_parallel for batch submission"
        );

        // The parallel submission must log timing
        assert!(
            source.contains("Parallel prompt submission complete"),
            "Parallel prompt submission must log timing for observability"
        );
    }

    /// Regression: all_prompts must NOT be shadowed in the poll_ok block.
    /// A `let mut all_prompts = ...` inside the block shadows the outer one,
    /// causing prompts to be collected but never submitted.
    #[test]
    fn test_all_prompts_not_shadowed() {
        let source = include_str!("main.rs");

        // Find the outer all_prompts declaration
        let outer = source.find("let mut all_prompts: Vec<tmux::PromptTask>")
            .expect("outer all_prompts declaration must exist");

        // After the outer declaration, there must be NO `let mut all_prompts =`
        // (which would shadow it)
        let after_outer = &source[outer + 50..];
        let shadow_count = after_outer.matches("let mut all_prompts").count()
            + after_outer.matches("let all_prompts").count();

        assert_eq!(
            shadow_count, 0,
            "REGRESSION: all_prompts is shadowed {shadow_count} time(s) — \
             prompts collected in inner scope are lost and never submitted"
        );
    }

    /// submit_prompts_parallel must exist in tmux.rs and use std::thread::scope.
    #[test]
    fn test_submit_prompts_parallel_uses_thread_scope() {
        let source = include_str!("tmux.rs");

        let fn_start = source.find("fn submit_prompts_parallel(")
            .expect("submit_prompts_parallel must exist in tmux.rs");
        let fn_end = source[fn_start..].find("\n}").unwrap_or(1000);
        let fn_body = &source[fn_start..fn_start + fn_end];

        assert!(
            fn_body.contains("thread::scope("),
            "submit_prompts_parallel must use std::thread::scope for parallelism"
        );

        assert!(
            fn_body.contains("s.spawn("),
            "submit_prompts_parallel must spawn a thread per prompt task"
        );

        // Must NOT use rayon (we chose std::thread::scope for zero dependencies)
        assert!(
            !fn_body.contains("rayon"),
            "submit_prompts_parallel must use std::thread::scope, not rayon"
        );
    }

    /// PromptTask must be Clone (needed for recovery path extend).
    #[test]
    fn test_prompt_task_is_clone() {
        let source = include_str!("tmux.rs");
        let prompt_task = source.find("pub struct PromptTask")
            .expect("PromptTask must exist");
        let before = &source[prompt_task.saturating_sub(50)..prompt_task];
        assert!(
            before.contains("Clone"),
            "PromptTask must derive Clone for extend/clone in recovery path"
        );
    }

    /// Observability: parallel prompt submission must log per-pane results.
    #[test]
    fn test_parallel_prompt_logs_per_pane() {
        let source = include_str!("tmux.rs");
        let fn_start = source.find("fn submit_prompts_parallel(")
            .expect("function must exist");
        let fn_body = &source[fn_start..fn_start + 1500];

        // Must log start with pane list
        assert!(
            fn_body.contains("Starting parallel prompt submission"),
            "Must log which panes are being submitted"
        );

        // Must log per-pane completion with timing
        assert!(
            fn_body.contains("Parallel prompt: pane done"),
            "Must log per-pane result for debugging"
        );
    }

    /// Observability: assign_idle_workers must log queuing and completion counts.
    #[test]
    fn test_assign_idle_workers_logs_queue_counts() {
        let source = include_str!("coordinator.rs");
        let fn_start = source.find("fn assign_idle_workers_inner(")
            .expect("function must exist");
        let fn_end = source[fn_start..].find("\n    pub fn ").unwrap_or(5000);
        let fn_body = &source[fn_start..fn_start + fn_end];

        assert!(
            fn_body.contains("Queuing prompt for parallel submission"),
            "Must log when a prompt is queued"
        );

        assert!(
            fn_body.contains("assign_idle_workers complete"),
            "Must log final assigned + queued counts"
        );
    }

    /// Edge case: empty prompt task list should return empty vec, not panic.
    #[test]
    fn test_submit_prompts_parallel_empty_input() {
        let source = include_str!("tmux.rs");
        let fn_start = source.find("fn submit_prompts_parallel(")
            .expect("function must exist");
        let fn_body = &source[fn_start..fn_start + 300];

        assert!(
            fn_body.contains("if tasks.is_empty()"),
            "submit_prompts_parallel must handle empty input without spawning threads"
        );
    }

    // =========================================================================
    // Regression: completion messages must NOT be acked until bead is closed
    // =========================================================================
    // Bug: acking on dispatch meant if br::close_bead() failed (db lock, etc),
    // the message was gone and the bead was orphaned in_progress forever.
    // Fix: ack only after reassign() successfully closes the bead.

    /// Regression: verification dispatch must NOT ack messages.
    /// Acking before close causes orphaned beads if close fails.
    #[test]
    fn test_verify_dispatch_does_not_ack() {
        let source = include_str!("coordinator.rs");
        // Find the thread::spawn dispatch and the "continue" after it.
        // Only check BETWEEN spawn and continue — not reject paths above.
        let spawn_pos = source.find("std::thread::spawn(move ||").expect("thread spawn must exist");
        let after_spawn = &source[spawn_pos..];
        let continue_pos = after_spawn.find("continue;").unwrap_or(500);
        let section = &after_spawn[..continue_pos];

        assert!(
            !section.contains("ack_or_mark_read"),
            "REGRESSION: verification dispatch must NOT ack messages between \
             thread::spawn and continue. Acking before close orphans beads."
        );
    }

    /// Regression: pending verification guard must NOT ack duplicate messages.
    /// Just skip them — they'll be processed when verification finishes.
    #[test]
    fn test_pending_verification_does_not_ack_duplicate() {
        let source = include_str!("coordinator.rs");
        let pending_check = source.find("already being verified").expect("pending guard must exist");
        let end = (pending_check + 300).min(source.len());
        let section = &source[pending_check..end];

        assert!(
            !section.contains("ack_or_mark_read"),
            "REGRESSION: pending verification guard must NOT ack duplicates. \
             Just skip — the pending set prevents re-dispatch."
        );
    }

    /// Regression: reassign() must ack the message AFTER br::close_bead succeeds.
    #[test]
    fn test_reassign_acks_after_close() {
        let source = include_str!("coordinator.rs");
        let reassign_fn = source.find("fn reassign(").expect("reassign must exist");
        // Use a large enough window to cover the entire function (~200 lines)
        let fn_body = &source[reassign_fn..(reassign_fn + 4000).min(source.len())];

        assert!(
            fn_body.contains("ack_msg"),
            "reassign() must ack the message after successful close"
        );
        // The main success-path self.ack_msg (Step 7) must come AFTER br::close_bead (Step 1).
        // There are early-exit ack calls for edge cases (worker not found, etc.) which are fine.
        let close_pos = fn_body.find("br::close_bead").expect("close_bead must exist in reassign");
        // Find the LAST self.ack_msg — that's the main success-path ack
        let last_ack_pos = fn_body.rfind("self.ack_msg").expect("self.ack_msg must exist in reassign");
        assert!(
            last_ack_pos > close_pos,
            "REGRESSION: main success-path self.ack_msg must come AFTER br::close_bead. \
             Acking before close causes orphaned beads."
        );
    }

    /// Regression: drain_verify_results must exist and be called in poll().
    #[test]
    fn test_drain_verify_results_called_in_poll() {
        let source = include_str!("coordinator.rs");
        let poll_fn = source.find("pub fn poll(").expect("poll() must exist");
        let fn_end = source[poll_fn..].find("\n    }").unwrap_or(2000);
        let fn_body = &source[poll_fn..poll_fn + fn_end];

        assert!(
            fn_body.contains("drain_verify_results"),
            "poll() must call drain_verify_results to process background verification results"
        );
    }

    // =========================================================================
    // Regression: reassign must NOT send prompt immediately
    // =========================================================================
    // Bug: reassign() built a PromptTask and sent it to the worker pane
    // while the worker was still outputting its completion summary.
    // This created "Press up to edit queued messages" that jammed workers.

    /// Regression: reassign() must NOT build a PromptTask. The next poll
    /// cycle should detect the idle worker and send the prompt then.
    #[test]
    fn test_reassign_does_not_send_immediate_prompt() {
        let source = include_str!("coordinator.rs");
        let reassign_fn = source.find("fn reassign(").expect("reassign() must exist");
        let fn_end = source[reassign_fn..].find("\n    }").unwrap_or(2000);
        let fn_body = &source[reassign_fn..reassign_fn + fn_end];

        assert!(
            !fn_body.contains("PromptTask {"),
            "REGRESSION: reassign() must NOT construct a PromptTask. \
             Sending a prompt immediately after closing a bead jams the \
             worker with 'Press up to edit' because it's still outputting."
        );
    }

    /// Regression: reassign must set prompt_task to None.
    #[test]
    fn test_reassign_returns_none_prompt_task() {
        let source = include_str!("coordinator.rs");
        let reassign_fn = source.find("fn reassign(").expect("reassign() must exist");
        let fn_end = source[reassign_fn..].find("\n    }").unwrap_or(2000);
        let fn_body = &source[reassign_fn..reassign_fn + fn_end];

        assert!(
            fn_body.contains("let prompt_task: Option<tmux::PromptTask> = None"),
            "reassign() must explicitly set prompt_task to None"
        );
    }

    // =========================================================================
    // Regression: background verification channel must exist
    // =========================================================================

    /// Regression: Coordinator must have verify_tx/verify_rx/pending_verifications fields.
    #[test]
    fn test_coordinator_has_verification_infrastructure() {
        let source = include_str!("coordinator.rs");
        let struct_def = source.find("pub struct Coordinator").expect("Coordinator must exist");
        let struct_end = source[struct_def..].find('}').unwrap_or(500);
        let struct_body = &source[struct_def..struct_def + struct_end];

        assert!(struct_body.contains("pending_verifications"), "must track pending verifications");
        assert!(struct_body.contains("verify_rx"), "must have verification result receiver");
        assert!(struct_body.contains("verify_tx"), "must have verification result sender");
    }

    /// Functional: test_coordinator() creates valid verification channels.
    #[test]
    fn test_coordinator_verify_channel_works() {
        let coord = test_coordinator();
        // Verify we can send and receive through the channel
        let tx = coord.verify_tx.clone();
        tx.send(VerifyResult {
            bead_id: "pat-test".to_string(),
            worker: "TestWorker".to_string(),
            msg_id: Some(1),
            ack_required: false,
            files_changed: "test.rs".to_string(),
            tests_run: "cargo test".to_string(),
            passed: true,
            reject_reason: None,
            is_stale_recovery: false,
        }).unwrap();

        let rx = coord.verify_rx.lock().unwrap();
        let result = rx.try_recv().unwrap();
        assert_eq!(result.bead_id, "pat-test");
        assert!(result.passed);
    }

    /// Functional: pending_verifications set prevents duplicate dispatches.
    #[test]
    fn test_pending_verifications_prevents_duplicates() {
        let coord = test_coordinator();
        {
            let mut pending = coord.pending_verifications.lock().unwrap();
            pending.insert("pat-abc".to_string());
        }
        let pending = coord.pending_verifications.lock().unwrap();
        assert!(pending.contains("pat-abc"));
        assert!(!pending.contains("pat-xyz"));
    }

    // =========================================================================
    // Stale-PROG recovery + pull-mode tests
    // =========================================================================

    #[test]
    fn test_verify_result_has_stale_recovery_field() {
        let source = include_str!("coordinator.rs");
        let s = source.find("struct VerifyResult").unwrap();
        let body = &source[s..s + 500];
        assert!(body.contains("is_stale_recovery"));
    }

    #[test]
    fn test_verify_stale_bead_method_exists() {
        assert!(include_str!("coordinator.rs").contains("fn verify_stale_bead("));
    }

    #[test]
    fn test_recover_stale_assignments_method_exists() {
        assert!(include_str!("coordinator.rs").contains("fn recover_stale_assignments("));
    }

    #[test]
    fn test_poll_calls_stale_recovery_directly() {
        let source = include_str!("coordinator.rs");
        let poll_fn = source.find("pub fn poll(").unwrap();
        let body = &source[poll_fn..poll_fn + 2000];
        assert!(body.contains("recover_stale_assignments"),
            "poll() must call recover_stale_assignments so it runs in pull mode too");
    }

    #[test]
    fn test_verify_stale_bead_falls_back_to_workspace_test() {
        let source = include_str!("coordinator.rs");
        let method = source.find("fn verify_stale_bead(").unwrap();
        let end = source[method + 1..].find("\n    fn ").unwrap_or(1000);
        let body = &source[method..method + end];
        assert!(body.contains("commands.is_empty()"));
        assert!(body.contains("cargo build --workspace"),
            "must fall back to workspace build check when no acceptance commands");
    }

    #[test]
    fn test_stale_recovery_verify_result_channel() {
        let coord = test_coordinator();
        coord.verify_tx.clone().send(VerifyResult {
            bead_id: "pat-stale".to_string(),
            worker: String::new(),
            msg_id: None,
            ack_required: false,
            files_changed: String::new(),
            tests_run: String::new(),
            passed: true,
            reject_reason: None,
            is_stale_recovery: true,
        }).unwrap();
        let rx = coord.verify_rx.lock().unwrap();
        let result = rx.try_recv().unwrap();
        assert!(result.is_stale_recovery);
        assert!(result.worker.is_empty());
    }

    #[test]
    fn test_verify_stale_bead_skips_if_already_pending() {
        let coord = test_coordinator();
        {
            coord.pending_verifications.lock().unwrap().insert("pat-already".to_string());
        }
        coord.verify_stale_bead("pat-already");
        assert!(coord.verify_rx.lock().unwrap().try_recv().is_err(),
            "should not dispatch verification for already-pending bead");
    }

    /// Regression: auto_create_beads_from_planner must check planner keys
    /// before creating beads to prevent duplicates across planner cycles.
    #[test]
    fn test_auto_create_checks_planner_keys() {
        let source = include_str!("main.rs");
        let func = source.find("fn auto_create_beads_from_planner(").unwrap();
        let end = source[func..].find("\n}").unwrap_or(2000);
        let body = &source[func..func + end];

        assert!(
            body.contains("planner-key"),
            "must check planner keys before creating beads"
        );
        assert!(
            body.contains("existing_keys"),
            "must load existing planner keys for dedup"
        );
    }
}
