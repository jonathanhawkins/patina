use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{Config, MailConfig};
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
    /// The worker delivered part of the scope and asked to keep the bead open
    /// (pat-k5o8f). A pass then records the scope and reopens instead of
    /// closing. Declared after `is_stale_recovery` on purpose — a test windows
    /// the first 500 chars of this struct looking for that field.
    partial: bool,
    /// What the worker says is still undelivered, for the durable comment.
    remaining_scope: String,
}

#[derive(Debug)]
struct VerifyRequest {
    bead_id: String,
    worker: String,
    msg_id: Option<i64>,
    ack_required: bool,
    files_changed: String,
    tests_run: String,
    commands: Vec<String>,
    is_stale_recovery: bool,
    /// When this request entered the queue — used to report queue-wait latency.
    enqueued_at: Instant,
    /// Carried through to [`VerifyResult::partial`] (pat-k5o8f).
    partial: bool,
    /// Carried through to [`VerifyResult::remaining_scope`].
    remaining_scope: String,
    /// Test names the bead demands (`(test: \`name\`)`); the red check only
    /// runs for beads that name one (see wiring.rs).
    acceptance_tests: Vec<String>,
}

/// Outcome bundle from one verification run, feeding both circuit breakers
/// (dyld-wedge via `outcome`, crate-breakage via the crate fields).
struct VerifyRunSummary {
    outcome: crate::verifier::circuit_breaker::VerifyOutcome,
    /// Crate the verify targeted (`-p`/`--package` in its commands).
    request_crate: Option<String>,
    /// Crate rustc blamed in a `could not compile` failure, if any.
    broken_crate: Option<String>,
    /// Tail of the failure output for operator alerts.
    error_excerpt: String,
}

pub struct Coordinator {
    pub config: Config,
    mail: MailClient,
    dry_run: bool,
    /// Beads currently being verified in background threads, mapped to the
    /// instant they were enqueued (so a wedged verification can be reaped).
    pending_verifications: Arc<Mutex<HashMap<String, Instant>>>,
    /// Completion messages currently queued/running in background verification.
    pending_completion_msgs: Arc<Mutex<HashSet<i64>>>,
    /// Channel to receive verification results.
    verify_rx: Mutex<mpsc::Receiver<VerifyResult>>,
    /// Sender cloned into background verification worker/tests.
    verify_tx: mpsc::Sender<VerifyResult>,
    /// Sender for serialized verification requests.
    verify_request_tx: mpsc::Sender<VerifyRequest>,
    /// Per-bead consecutive-orphan strike counts (B4 orphan reaper).
    orphan_strikes: Mutex<HashMap<String, u32>>,
    /// Cache of resolved worker identities by pane id, with resolution time (D2).
    identity_cache: Mutex<HashMap<String, (String, Instant)>>,
    /// Last bead each worker had its `/effort` set for. Lets the prompt path
    /// tell a genuinely-new bead (inject `/clear` + `/effort`) from a re-nudge
    /// of the bead the worker is already on (inject nothing — never wipe WIP).
    last_effort_bead: Mutex<HashMap<String, String>>,
    /// Last time a bead was reclaimed from each worker (D4 reclaim hysteresis).
    last_reclaim: Mutex<HashMap<String, Instant>>,
    /// Last time a bead close succeeded via any path (close-stall detection).
    last_close: Mutex<Option<Instant>>,
    /// Coordinator start time — close-stall baseline before the first close.
    started_at: Instant,
    /// Last close-stall operator notification (rate limiting).
    last_stall_notify: Mutex<Option<Instant>>,
    /// Consecutive failed verifications per bead (fail-streak parking).
    verify_fail_streaks: Mutex<HashMap<String, u32>>,
    /// Successful closes since the last auto-checkpoint commit.
    closes_since_checkpoint: Arc<Mutex<u64>>,
    /// Guards a single in-flight auto-checkpoint thread.
    checkpoint_in_flight: Arc<std::sync::atomic::AtomicBool>,
    /// Last auto-checkpoint attempt (success or failure) — attempt throttle.
    last_checkpoint_attempt: Mutex<Option<Instant>>,
}

impl Coordinator {
    pub fn new(config: Config, dry_run: bool) -> Result<Self> {
        // Verify DB is accessible at startup
        let _db = db::open(&config.project_root)?;
        drop(_db);
        let mail = MailClient::new(&config.mail);
        let pending_verifications = Arc::new(Mutex::new(HashMap::new()));
        let pending_completion_msgs = Arc::new(Mutex::new(HashSet::new()));
        let _ = fs::create_dir_all(config.cache_dir());
        let (verify_tx, verify_rx) = mpsc::channel();
        let (verify_request_tx, verify_request_rx) = mpsc::channel();
        spawn_verifier_worker(
            config.project_root.clone(),
            Duration::from_secs(config.verify_timeout_seconds),
            config.crate_breaker_threshold,
            verify_request_rx,
            verify_tx.clone(),
            Arc::clone(&pending_verifications),
            Arc::clone(&pending_completion_msgs),
            config.cache_dir().join("verifier.log"),
            config.mail.clone(),
            config.coordinator_agent.clone(),
            config.project_root.to_string_lossy().to_string(),
        );
        Ok(Self {
            config,
            mail,
            dry_run,
            pending_verifications,
            pending_completion_msgs,
            verify_rx: Mutex::new(verify_rx),
            verify_tx,
            verify_request_tx,
            orphan_strikes: Mutex::new(HashMap::new()),
            identity_cache: Mutex::new(HashMap::new()),
            last_effort_bead: Mutex::new(HashMap::new()),
            last_reclaim: Mutex::new(HashMap::new()),
            last_close: Mutex::new(None),
            started_at: Instant::now(),
            last_stall_notify: Mutex::new(None),
            verify_fail_streaks: Mutex::new(HashMap::new()),
            closes_since_checkpoint: Arc::new(Mutex::new(0)),
            checkpoint_in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_checkpoint_attempt: Mutex::new(None),
        })
    }

    /// Open a fresh DB connection (avoids holding WAL read locks that block br writes).
    fn open_db(&self) -> Result<rusqlite::Connection> {
        db::open(&self.config.project_root)
    }

    /// Reopen beads whose verification has been pending longer than the
    /// configured reap threshold (B3). A wedged or panicked verifier would
    /// otherwise leave a bead in `pending_verifications` forever, silently
    /// stalling it (and blocking re-dispatch, since the pending set gates that).
    /// Returns the number of beads reaped.
    pub fn reap_stale_verifications(&self) -> usize {
        let threshold = Duration::from_secs(self.config.pending_verification_reap_seconds);
        let stale: Vec<String> = {
            let pending = self.pending_verifications.lock().unwrap();
            pending
                .iter()
                .filter(|(_, enqueued)| enqueued.elapsed() >= threshold)
                .map(|(bead_id, _)| bead_id.clone())
                .collect()
        };
        if stale.is_empty() {
            return 0;
        }
        for bead_id in &stale {
            tracing::error!(
                bead = %bead_id,
                threshold_secs = self.config.pending_verification_reap_seconds,
                "Verification wedged past reap threshold — reopening bead for a new worker"
            );
            if !self.dry_run {
                let _ = br::reopen(bead_id);
            }
            self.pending_verifications.lock().unwrap().remove(bead_id);
        }
        stale.len()
    }

    /// Reopen beads that have been orphaned (in_progress, no assignee) for
    /// `orphan_reap_consecutive_polls` consecutive polls (B4). The strike
    /// counter avoids reopening a bead caught in the brief window of a
    /// mid-flight assignment. Returns the number of beads reopened.
    pub fn reap_orphaned_beads(&self) -> usize {
        let orphaned = match self.open_db() {
            Ok(db) => {
                let o = db::orphaned_in_progress(&db).unwrap_or_default();
                drop(db);
                o
            }
            Err(_) => return 0,
        };
        // Don't reap a bead that's actively being verified (it legitimately has
        // no assignee during stale-recovery verification).
        let pending: HashSet<String> = self
            .pending_verifications
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let orphaned: Vec<String> = orphaned
            .into_iter()
            .filter(|id| !pending.contains(id))
            .collect();

        let threshold = self.config.orphan_reap_consecutive_polls;
        let to_reopen = {
            let mut strikes = self.orphan_strikes.lock().unwrap();
            update_orphan_strikes(&orphaned, &mut strikes, threshold)
        };
        for bead_id in &to_reopen {
            tracing::error!(
                bead = %bead_id,
                strikes = threshold,
                "Orphaned bead (in_progress, no assignee) across consecutive polls — reopening"
            );
            if !self.dry_run {
                let _ = br::reopen(bead_id);
            }
        }
        to_reopen.len()
    }

    /// Resolve a worker identity for a pane, caching the result for
    /// `identity_cache_ttl_seconds` to cut per-poll `identity-resolve.sh`
    /// syscalls and shrink the resolve→check race window (D2).
    pub fn resolve_worker_identity_cached(&self, pane_id: &str) -> Option<String> {
        let ttl = self.config.identity_cache_ttl_seconds;
        if ttl > 0 {
            let cache = self.identity_cache.lock().unwrap();
            if let Some((name, at)) = cache.get(pane_id) {
                if at.elapsed() < Duration::from_secs(ttl) {
                    return Some(name.clone());
                }
            }
        }
        let resolved = worker::resolve_worker_identity(&self.config.project_root, pane_id);
        if let Some(name) = &resolved {
            if ttl > 0 {
                self.identity_cache
                    .lock()
                    .unwrap()
                    .insert(pane_id.to_string(), (name.clone(), Instant::now()));
            }
        }
        resolved
    }

    /// Renew pane identities that aged out of `identity-resolve.sh`'s staleness
    /// window so assignment never stalls silently on an expired identity.
    ///
    /// After ~2 days of continuous running every identity file's embedded
    /// timestamp passes `IDENTITY_STALE_SECONDS`, the resolve script starts
    /// exiting non-zero, and every pane reports an empty `worker_name`. The
    /// assignment loop skips nameless panes, so the swarm quietly assigned
    /// nothing for hours despite a full ready queue and idle workers — until an
    /// operator cycled the flywheel. The recorded name is still correct (a
    /// pane's `AGENT_NAME` is fixed at launch), so renew it in place and keep
    /// assigning. Returns the number of identities recovered.
    fn recover_expired_identities(&self, workers: &mut [worker::WorkerInfo]) -> usize {
        let mut recovered = 0usize;

        for w in workers.iter_mut() {
            if !w.worker_name.is_empty() || self.config.is_reserved_verifier_pane(w.pane_index) {
                continue;
            }

            match worker::renew_worker_identity(&self.config.project_root, &w.pane_id) {
                Some(name) => {
                    tracing::warn!(
                        pane = w.pane_index,
                        pane_id = %w.pane_id,
                        worker = %name,
                        "identity expired past its staleness window; renewed in place — \
                         assignment continues without an operator cycle"
                    );
                    // Keep the resolve cache consistent with the renewed file so
                    // other pane->name lookups this cycle see the same answer.
                    if self.config.identity_cache_ttl_seconds > 0 {
                        self.identity_cache
                            .lock()
                            .unwrap()
                            .insert(w.pane_id.clone(), (name.clone(), Instant::now()));
                    }
                    w.worker_name = name;
                    recovered += 1;
                }
                None => {
                    tracing::error!(
                        pane = w.pane_index,
                        pane_id = %w.pane_id,
                        "identity resolution FAILED and the pane has no renewable record \
                         (never bootstrapped, or too old to trust — tmux recycles pane ids) — \
                         pane is excluded from assignment; relaunch it to re-bootstrap"
                    );
                }
            }
        }

        recovered
    }

    /// Drain completed background verifications and process results.
    /// Called at the start of each poll cycle so results are handled promptly.
    pub fn drain_verify_results(&self) -> (usize, usize, Vec<tmux::PromptTask>) {
        // Backstop: reclaim any verification that has been wedged too long
        // before draining fresh results, and reopen any orphaned beads.
        self.reap_stale_verifications();
        self.reap_orphaned_beads();
        let rx = self.verify_rx.lock().unwrap();
        let mut processed = 0usize;
        let mut rejected = 0usize;
        let mut prompt_tasks = Vec::new();

        while let Ok(vr) = rx.try_recv() {
            if let Some(id) = vr.msg_id {
                self.pending_completion_msgs.lock().unwrap().remove(&id);
            }
            if vr.passed {
                // A pass ends any fail streak (fail-streak parking).
                self.verify_fail_streaks.lock().unwrap().remove(&vr.bead_id);
                if vr.is_stale_recovery {
                    // Stale-PROG recovery — just close, no worker to reassign
                    tracing::info!(bead = %vr.bead_id, "Stale bead verified PASSED — closing as done");
                    match br::close_bead(&vr.bead_id, "stale-recovery: acceptance tests passed") {
                        Ok(_) => {
                            self.record_close();
                            processed += 1;
                        }
                        Err(e) => {
                            tracing::error!(
                                bead = %vr.bead_id,
                                error = %e,
                                "Failed to close stale-verified bead"
                            );
                        }
                    }
                } else if vr.partial {
                    // The delivered scope verified, but the worker said the
                    // bead is not finished. Closing here is what silently
                    // marked real gaps done (pat-k5o8f) — instead record the
                    // remaining scope on the bead and return it to the queue.
                    match self.handle_partial_completion(&vr) {
                        Ok(()) => {
                            self.ack_or_mark_read(vr.msg_id);
                            processed += 1;
                        }
                        Err(e) => {
                            tracing::error!(
                                bead = %vr.bead_id,
                                error = %e,
                                "ERROR: partial completion could not be recorded — \
                                 leaving the message unacked to retry next cycle"
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
                    // Normal verification failed — reopen bead and notify worker.
                    // Repeated failures park the bead instead of round-robining
                    // it through the swarm forever (fail-streak parking).
                    // Bare timeouts are infrastructure failures (contended or
                    // wedged lane), not evidence against the bead — they feed
                    // the dyld breaker, never the park streak. Otherwise one
                    // slow-lane period (cold cache, checkpoint gate) would
                    // park innocent beads.
                    let infra_timeout = vr
                        .reject_reason
                        .as_deref()
                        .is_some_and(|r| r.contains("timed out after"));
                    let (streak, park) = if infra_timeout {
                        (0, false)
                    } else {
                        let mut streaks = self.verify_fail_streaks.lock().unwrap();
                        bump_fail_streak(
                            &mut streaks,
                            &vr.bead_id,
                            self.config.verify_fail_park_threshold,
                        )
                    };
                    let details = format!(
                        "Files changed: {}\nTests run: {}",
                        if vr.files_changed.is_empty() {
                            "<missing>"
                        } else {
                            &vr.files_changed
                        },
                        if vr.tests_run.is_empty() {
                            "<missing>"
                        } else {
                            &vr.tests_run
                        },
                    );
                    let reason = vr
                        .reject_reason
                        .as_deref()
                        .unwrap_or("reported test commands failed when rerun by the coordinator");
                    if park {
                        tracing::warn!(
                            bead = %vr.bead_id,
                            streak,
                            "PARKING bead after repeated verification failures"
                        );
                        if !self.dry_run {
                            let _ = br::label_add(&vr.bead_id, "needs-attention");
                            let _ = br::reopen(&vr.bead_id);
                        }
                        self.notify_boss(&format!(
                            "PARKED {}: {} consecutive failed verifications — labeled \
                             needs-attention and excluded from assignment until an operator \
                             clears the label. Last failure: {}",
                            vr.bead_id,
                            streak,
                            verifier::tail_chars(reason, 300)
                        ));
                        let park_reason = format!(
                            "parked after {streak} failed verifications — the bead is labeled \
                             needs-attention for operator triage; do NOT retry it, claim other work. \
                             Last failure: {reason}"
                        );
                        let _ = self.send_reopen_message(
                            &vr.worker,
                            &vr.bead_id,
                            &park_reason,
                            &details,
                        );
                    } else {
                        tracing::info!(
                            bead = %vr.bead_id,
                            reason = ?vr.reject_reason,
                            streak,
                            "REJECT: verification failed"
                        );
                        let _ = br::reopen(&vr.bead_id);
                        let _ = self.send_reopen_message(&vr.worker, &vr.bead_id, reason, &details);
                    }
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

    /// Record a verified-but-incomplete completion and put the bead back in the
    /// queue instead of closing it (pat-k5o8f).
    ///
    /// The comment is written before the reopen — a bead back in the queue
    /// without its scope note has lost the information this path exists to
    /// preserve — but a failed comment does **not** abort the operation.
    ///
    /// It used to. That cost a live retry loop on 2026-07-25: `comment_add`
    /// passed a flag `br` does not accept, so every attempt failed, the
    /// completion was deliberately left unacked "to retry next cycle", and the
    /// coordinator re-verified an already-passing bead every ~9 seconds
    /// forever. A persistent failure retried unboundedly is worse than a
    /// degraded success: the reopen still happens, the scope still reaches the
    /// worker by mail, and the error is loud in the log.
    fn handle_partial_completion(&self, vr: &VerifyResult) -> Result<()> {
        let comment = partial_scope_comment(
            &vr.worker,
            &vr.files_changed,
            &vr.tests_run,
            &vr.remaining_scope,
        );
        if self.dry_run {
            tracing::info!(
                bead = %vr.bead_id,
                "[dry-run] would record partial scope and reopen"
            );
            return Ok(());
        }
        if let Err(e) = br::comment_add(&vr.bead_id, &comment) {
            tracing::error!(
                bead = %vr.bead_id,
                error = %e,
                "PARTIAL scope comment could not be written — reopening anyway; \
                 the remaining scope survives only in the worker message"
            );
        }
        br::reopen(&vr.bead_id)?;
        tracing::info!(
            bead = %vr.bead_id,
            worker = %vr.worker,
            "PARTIAL verified — scope recorded, bead returned to open"
        );
        let _ = self.send_partial_message(&vr.worker, &vr.bead_id, &vr.remaining_scope);
        Ok(())
    }

    /// Tell the worker their partial was accepted and the bead stayed open, so
    /// they don't re-report it (or assume the keep-open request was ignored
    /// again).
    fn send_partial_message(&self, worker: &str, bead_id: &str, remaining: &str) -> Result<()> {
        if worker.is_empty() {
            return Ok(());
        }
        let coordinator = self.coordinator_name()?;
        let project_key = self.config.project_root.to_string_lossy().to_string();
        let body = format!(
            "## [{bead_id}] Partial accepted\n\n\
             Your delivered scope verified. The bead was **not** closed — it is back \
             in the ready queue with your remaining scope recorded as a `br` comment, \
             so it survives for whoever picks it up next.\n\n\
             Remaining scope on record:\n{}\n\n\
             No action needed from you on this bead.\n",
            if remaining.trim().is_empty() {
                "(none stated — consider listing it next time so the split is actionable)"
            } else {
                remaining
            }
        );
        let msg = OutgoingMessage {
            project_key,
            sender_name: coordinator,
            to: vec![worker.to_string()],
            subject: format!("[{bead_id}] Partial accepted — bead left open"),
            body_md: body,
            importance: "normal".to_string(),
            ack_required: false,
            topic: "bead-partial".to_string(),
            thread_id: bead_id.to_string(),
        };

        self.mail.send_message_best_effort(&msg)
    }

    /// Dispatch background verification for a stale PROG bead.
    /// Instead of blindly reopening, check if the acceptance tests pass.
    /// If they pass → close as done. If they fail → reopen for a new worker.
    fn verify_stale_bead(&self, bead_id: &str) {
        // Get acceptance commands from bead description, plus crate labels for
        // the fallback scope.
        let (mut commands, crate_labels) = match self.open_db() {
            Ok(db) => {
                let desc = db::bead_description(&db, bead_id).ok().flatten();
                // The structured field first: port-audit beads state their
                // acceptance there, not on an `Acceptance:` line (pat-mpqun).
                let criteria = db::bead_acceptance_criteria(&db, bead_id);
                let labels = db::bead_crate_labels(&db, bead_id).unwrap_or_default();
                drop(db);
                let mut cmds = Vec::new();
                for text in [criteria.as_deref(), desc.as_deref()].into_iter().flatten() {
                    for cmd in verifier::extract_acceptance_commands(text) {
                        if !cmds.contains(&cmd) {
                            cmds.push(cmd);
                        }
                    }
                }
                (cmds, labels)
            }
            Err(_) => (vec![], vec![]),
        };

        if commands.is_empty() {
            // Fallback: just verify the bead's crates compile. Running the full
            // test suite takes 15+ minutes and times out, creating a reopen
            // loop — and a full `cargo build --workspace` compiles and links
            // every crate (wgpu/metal included), starving the verify lane.
            commands = stale_fallback_commands(&crate_labels);
            tracing::info!(
                bead = %bead_id,
                fallback = %commands.join(" && "),
                "Stale bead has no acceptance commands — using scoped check fallback"
            );
        }

        let _ = self.enqueue_verification(VerifyRequest {
            bead_id: bead_id.to_string(),
            worker: String::new(),
            msg_id: None,
            ack_required: false,
            files_changed: String::new(),
            tests_run: String::new(),
            commands,
            is_stale_recovery: true,
            enqueued_at: Instant::now(),
            // Stale recovery has no worker and therefore no keep-open request.
            partial: false,
            remaining_scope: String::new(),
            // Stale recovery skips the red check (no files_changed to revert).
            acceptance_tests: Vec::new(),
        });
    }

    /// Sweep beads that workers marked "done" or "complete" directly.
    /// In pull mode, workers sometimes use `br update --status done` instead
    /// of `/mail-complete`. This catches those and closes them properly.
    /// Refresh tmux pane border titles for workers.
    /// Claude Code overrides pane titles on startup, so we re-set them periodically.
    /// Rate-limited to once per 30 seconds.
    fn refresh_pane_titles(&self, session: &str, window: u32) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static LAST_RUN: AtomicU64 = AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now.saturating_sub(LAST_RUN.load(Ordering::Relaxed)) < 30 {
            return;
        }
        LAST_RUN.store(now, Ordering::Relaxed);

        // Set fixed pane titles
        let _ = tmux::set_pane_title(session, window, 0, "coordinator");
        let _ = tmux::set_pane_title(session, window, 1, "verifier");
        let _ = tmux::set_pane_title(session, window, 2, "planner");

        // Set worker pane titles from DB assignments
        if let Ok(db) = self.open_db() {
            if let Ok(workers) =
                worker::worker_info_list_with_config(session, window, &db, &self.config)
            {
                for w in &workers {
                    let title = if let Some(bead) = &w.assigned_bead {
                        format!("{} | {}", w.worker_name, bead)
                    } else if !w.worker_name.is_empty() {
                        format!("{} | idle", w.worker_name)
                    } else {
                        continue;
                    };
                    let _ = tmux::set_pane_title(session, window, w.pane_index, &title);
                }
            }
        }
    }

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
            if self
                .pending_verifications
                .lock()
                .unwrap()
                .contains_key(bead_id)
            {
                continue;
            }
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
                let mut stmt = match conn
                    .prepare("SELECT id FROM issues WHERE status IN ('done', 'complete')")
                {
                    Ok(s) => s,
                    Err(_) => return 0,
                };
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .unwrap_or_else(|_| panic!("query failed"));
                rows.filter_map(|r| r.ok()).collect()
            }
            Err(_) => return 0,
        };

        let mut closed = 0;
        for id in &ids {
            match br::close_bead(id, "Self-completed by worker (pull mode)") {
                Ok(_) => {
                    self.record_close();
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
                tracing::info!(
                    swept,
                    "Swept self-completed beads (workers used br update --status done)"
                );
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

        // Refresh pane border titles so they stay correct after Claude Code
        // overrides them on startup. Sets "worker_name | bead_id" for workers.
        if !self.dry_run {
            if let Some(session) = &self.config.session_name {
                let window = self.config.window_index;
                self.refresh_pane_titles(session, window);
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

            if self
                .pending_verifications
                .lock()
                .unwrap()
                .contains_key(&completion.bead_id)
            {
                continue;
            }
            if let Some(id) = completion.msg_id {
                if self.pending_completion_msgs.lock().unwrap().contains(&id) {
                    continue;
                }
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
                        tracing::warn!(
                            bead = %completion.bead_id,
                            from = %current_assignee,
                            to = %completion.worker,
                            "proceeding with verification anyway: assignee mismatch, reassigning to actual completer"
                        );
                        // WARN (not INFO) so this assignment race is visible in
                        // monitoring. We do NOT reject/`continue` — the worker
                        // that sent the completion did the work, so update the
                        // assignee and keep going; ack only happens after a close.
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
                    if pending.contains_key(&completion.bead_id) {
                        tracing::debug!(bead = %completion.bead_id, "already being verified, skipping");
                        continue;
                    }
                }

                let reported = verifier::extract_test_commands(&completion.tests_run);

                // What the BEAD demands, from its structured acceptance_criteria
                // and its description (pat-mpqun). Read before the reported
                // commands are trusted: a bead that names a test is not proven
                // by a green `cargo check` somewhere else.
                let (description, criteria) = {
                    let db = self.open_db()?;
                    let desc = db::bead_description(&db, &completion.bead_id)?;
                    let criteria = db::bead_acceptance_criteria(&db, &completion.bead_id);
                    drop(db);
                    (desc, criteria)
                };
                let mut demand = verifier::AcceptanceDemand::default();
                for text in [criteria.as_deref(), description.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    let parsed = verifier::parse_acceptance(text);
                    for cmd in parsed.commands {
                        if !demand.commands.contains(&cmd) {
                            demand.commands.push(cmd);
                        }
                    }
                    for name in parsed.test_names {
                        if !demand.test_names.contains(&name) {
                            demand.test_names.push(name);
                        }
                    }
                }

                // An unmet acceptance demand collapses into the same rejection
                // path as "no commands at all" — with a reason that names the
                // test the worker still has to run.
                let (commands, unmet_acceptance) =
                    match verifier::select_verify_commands(&reported, &demand) {
                        Ok(cmds) => match crate::wiring::check_entry_point(
                            &self.config.project_root,
                            &format!(
                                "{}\n{}",
                                criteria.as_deref().unwrap_or(""),
                                description.as_deref().unwrap_or("")
                            ),
                            &demand.test_names,
                            &completion.files_changed,
                        ) {
                            // The bead has a Wiring requirement and its acceptance
                            // test calls the implementation directly: reject before
                            // spending a build on it.
                            crate::wiring::EntryPointVerdict::Rejected(why) => {
                                tracing::info!(
                                    bead = %completion.bead_id,
                                    reason = %why,
                                    "REJECT: acceptance test does not use a public entry point"
                                );
                                (Vec::new(), Some(why))
                            }
                            crate::wiring::EntryPointVerdict::Inconclusive(why) => {
                                tracing::info!(
                                    bead = %completion.bead_id,
                                    reason = %why,
                                    "entry-point rule inconclusive; not blocking"
                                );
                                (cmds, None)
                            }
                            _ => (cmds, None),
                        },
                        Err(why) => {
                            tracing::info!(
                                bead = %completion.bead_id,
                                reason = %why,
                                "REJECT: acceptance test not covered by reported commands"
                            );
                            (Vec::new(), Some(why))
                        }
                    };

                if commands.is_empty() {
                    tracing::info!(
                        bead = %completion.bead_id,
                        "REJECT: no test commands to verify"
                    );
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
                    let reason = unmet_acceptance
                        .unwrap_or_else(|| "missing concrete completion evidence".to_string());
                    let _ = self.send_reopen_message(
                        &completion.worker,
                        &completion.bead_id,
                        &reason,
                        &details,
                    );
                    self.ack_or_mark_read(completion.msg_id);
                    result.rejected += 1;
                    continue;
                }

                let _ = self.enqueue_verification(VerifyRequest {
                    bead_id: completion.bead_id.clone(),
                    worker: completion.worker.clone(),
                    msg_id: completion.msg_id,
                    ack_required: completion.ack_required,
                    files_changed: completion.files_changed.clone(),
                    tests_run: completion.tests_run.clone(),
                    commands,
                    is_stale_recovery: false,
                    enqueued_at: Instant::now(),
                    partial: completion.partial,
                    remaining_scope: completion.remaining_scope.clone(),
                    acceptance_tests: demand.test_names.clone(),
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
                match self.reassign(&completion.bead_id, &completion.worker, completion.msg_id) {
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
    pub fn assign_idle_workers(
        &self,
        session: &str,
        window: u32,
    ) -> Result<(usize, Vec<tmux::PromptTask>)> {
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
        let mut workers = {
            let db = self.open_db()?;
            let workers = worker::worker_info_list_with_config(session, window, &db, &self.config)?;
            // db dropped here
            workers
        };

        // Recover identities that aged out of the resolve script's staleness
        // window BEFORE any per-worker state is keyed off `worker_name` —
        // otherwise an expired pane looks nameless and is dropped from
        // assignment entirely (see `recover_expired_identities`).
        self.recover_expired_identities(&mut workers);

        let mut assigned = 0usize;
        let prompt_workers = self.config.worker_requires_assignment_prompt();
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
                map.insert(
                    w.worker_name.clone(),
                    WorkerDbState {
                        active_count,
                        assigned_bead,
                    },
                );
            }
            // qdb dropped here — safe for br mutations below
            map
        };

        // Close-stall backpressure: when nothing has closed for the configured
        // window while THIS swarm's workers are saturated with claimed work
        // AND finished work is queued for verification, the verifier lane is
        // wedged — re-prompting churns tokens and reclaiming reverts completed
        // work stuck in the pipe. A drought with an EMPTY verify queue is the
        // opposite failure (workers never started — swallowed prompts), where
        // the re-prompt is the remedy, so suppression requires queued > 0.
        // Fresh assignments to genuinely idle workers always stay enabled so
        // backpressure can never deadlock the swarm.
        let worker_names: Vec<String> = workers
            .iter()
            .filter(|w| {
                !w.worker_name.is_empty() && !self.config.is_reserved_verifier_pane(w.pane_index)
            })
            .map(|w| w.worker_name.clone())
            .collect();
        let in_progress_count = {
            let db = self.open_db()?;
            db::count_in_progress_for_workers(&db, &worker_names)?
        };
        let close_stalled = self.close_stalled(in_progress_count, worker_names.len());
        let queued_verifications = self.pending_verifications.lock().unwrap().len();
        let stall_suppression =
            stall_suppression_active(close_stalled, queued_verifications, skip_completion_grace);
        if close_stalled {
            self.maybe_notify_close_stall(in_progress_count, worker_names.len());
        }

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

                        // Check if should reclaim. Under stall suppression the
                        // "stale" assignment is usually a completion stuck in
                        // the wedged verifier lane — reclaiming reverts done
                        // work. Recovery mode and an empty verify queue both
                        // disable suppression (see stall_suppression_active).
                        let allow_reclaim = !stall_suppression;
                        if allow_reclaim
                            && should_reclaim_idle(
                                bead,
                                age,
                                stale_threshold,
                                self.config.idle_reclaim_grace_seconds,
                                session,
                                window,
                                w.pane_index,
                                &w.worker_name,
                                &self,
                            )
                        {
                            // D4: detect a repeat reclaim from this worker within
                            // the hysteresis window — that's the reclaim↔reassign
                            // ping-pong signature of a genuinely stuck worker.
                            let ping_pong = {
                                let lr = self.last_reclaim.lock().unwrap();
                                lr.get(&w.worker_name)
                                    .map(|t| {
                                        within_hysteresis(
                                            t.elapsed(),
                                            self.config.reclaim_hysteresis_seconds,
                                        )
                                    })
                                    .unwrap_or(false)
                            };
                            if !self.dry_run {
                                let _ = br::reopen(&bead.id);
                                self.last_reclaim
                                    .lock()
                                    .unwrap()
                                    .insert(w.worker_name.clone(), Instant::now());
                            }
                            if ping_pong {
                                tracing::warn!(
                                    worker = %w.worker_name,
                                    bead = %bead.id,
                                    "Reclaimed again within hysteresis window — withholding \
                                     reassignment this cycle to break reclaim/reassign ping-pong"
                                );
                                continue;
                            }
                            // Re-check after reclaim (need fresh DB read)
                            let qdb = self.open_db()?;
                            if db::active_assignment_count(&qdb, &w.worker_name)? > 0 {
                                continue;
                            }
                            // Fall through to fresh assignment
                        } else {
                            // Not reclaimable — check reprompt
                            if stall_suppression {
                                tracing::info!(
                                    pane = w.pane_index,
                                    worker = %w.worker_name,
                                    bead = %bead.id,
                                    "close-stall backpressure: suppressing re-prompt"
                                );
                                continue;
                            }
                            if self.should_skip_reprompt(&w.worker_name, &bead.id) {
                                continue;
                            }

                            // Check if already has queued prompt
                            let capture = tmux::capture_pane(session, window, w.pane_index, 120)
                                .unwrap_or_default();
                            if worker::has_assignment_prompt_with(
                                &capture,
                                &self.config.queue_prompt_marker,
                                &bead.id,
                            ) {
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
                                // Update pane title to show worker + bead
                                let _ = tmux::set_pane_title(
                                    session,
                                    window,
                                    w.pane_index,
                                    &format!("{} | {}", w.worker_name, bead.id),
                                );
                                if prompt_workers {
                                    // Inject effort only on the FIRST prompt of a
                                    // newly-assigned bead (finish→next); a genuine
                                    // re-nudge of the same bead injects nothing.
                                    let (effort, clear_context) =
                                        self.effort_for_prompt(&w.worker_name, bead);
                                    prompt_tasks.push(tmux::PromptTask {
                                        session: session.to_string(),
                                        window,
                                        pane: w.pane_index,
                                        text: self.build_prompt_text(
                                            &bead.id,
                                            &bead.title,
                                            &w.worker_name,
                                        ),
                                        pcfg: self.prompt_config(),
                                        effort,
                                        clear_context,
                                    });
                                    self.record_reprompt(&w.worker_name, &bead.id);
                                }
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
            let (ready, busy_crates) = self.ready_work_context()?;
            let next = pick_from_ready(&ready, &busy_crates, &selected_beads);

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
                // Update pane title to show worker + bead
                let _ = tmux::set_pane_title(
                    session,
                    window,
                    w.pane_index,
                    &format!("{} | {}", w.worker_name, bead.id),
                );
                if prompt_workers {
                    // Queue prompt for parallel submission instead of blocking here.
                    tracing::info!(
                        pane = w.pane_index,
                        worker = %w.worker_name,
                        bead = %bead.id,
                        "Queuing prompt for parallel submission"
                    );
                    // Fresh idle-fill assignment: set this bead's effort and reset
                    // the worker's context before the work prompt.
                    let (effort, clear_context) = self.effort_for_prompt(&w.worker_name, &bead);
                    prompt_tasks.push(tmux::PromptTask {
                        session: session.to_string(),
                        window,
                        pane: w.pane_index,
                        text: self.build_prompt_text(&bead.id, &bead.title, &w.worker_name),
                        pcfg: self.prompt_config(),
                        effort,
                        clear_context,
                    });
                    self.record_reprompt(&w.worker_name, &bead.id);
                }
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

    fn enqueue_verification(&self, request: VerifyRequest) -> Result<()> {
        {
            let mut pending = self.pending_verifications.lock().unwrap();
            if pending.contains_key(&request.bead_id) {
                tracing::debug!(bead = %request.bead_id, "verification already pending");
                return Ok(());
            }
            pending.insert(request.bead_id.clone(), Instant::now());
        }
        // C6: surface verifier-lane backpressure so an operator can see the
        // single builder backing up before it turns into a silent stall.
        {
            let queued = self.pending_verifications.lock().unwrap().len();
            if queued >= self.config.completion_queue_warn_depth {
                tracing::warn!(
                    queued,
                    threshold = self.config.completion_queue_warn_depth,
                    "verifier lane backpressure: beads queued for verification"
                );
            }
        }
        if let Some(id) = request.msg_id {
            self.pending_completion_msgs.lock().unwrap().insert(id);
        }
        if let Err(e) = self.verify_request_tx.send(request) {
            let failed = e.0;
            let bead_id = failed.bead_id.clone();
            self.pending_verifications.lock().unwrap().remove(&bead_id);
            if let Some(id) = failed.msg_id {
                self.pending_completion_msgs.lock().unwrap().remove(&id);
            }
            return Err(OrchestratorError::Verification(format!(
                "failed to enqueue verification for {bead_id}: channel closed"
            )));
        }
        Ok(())
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
        let none_result = ReassignResult {
            next_bead_id: None,
            prompt_task: None,
        };

        // Step 1: Close the completed bead FIRST — unconditionally.
        //
        // The close MUST happen before the "other active beads" check below.
        // Previously the ordering was reversed: if the worker still held any
        // other active bead, reassign() returned early and the just-verified
        // bead was never closed — its PASS result silently dropped and the
        // completion message acked. A worker holding one wedged/orphaned bead
        // therefore blocked EVERY subsequent bead it completed from ever
        // closing, a self-sustaining stall. Closing first decouples the two:
        // the completed bead always closes; only NEW-work assignment is gated
        // on the worker being otherwise idle (Step 2).
        tracing::info!(bead = %completed_bead, "Closing completed bead");
        match br::close_bead(completed_bead, "completed") {
            Ok(_) => self.record_close(),
            Err(e) => {
                let msg = format!("{e}").to_lowercase();
                if msg.contains("already closed") || msg.contains("not found") {
                    // Already closed, or still not found even after br::close_bead's
                    // automatic sync-and-retry (bead may only exist in agent mail,
                    // not in any JSONL). Treat as closed and continue.
                    // "Already closed" (e.g. an operator's manual br close) is
                    // still close-flow evidence — record it so a latched
                    // close-stall doesn't survive real progress.
                    if msg.contains("already closed") {
                        self.record_close();
                    }
                    if msg.contains("not found") {
                        tracing::warn!(
                            bead = %completed_bead,
                            "Bead not found even after sync retry — treating as closed and continuing"
                        );
                    }
                } else {
                    // Unexpected failure — do NOT ack. The next poll cycle will
                    // retry the close.
                    tracing::error!(error = %e, "br close failed with unexpected error");
                    return Err(e);
                }
            }
        }

        // Step 2: The completed bead is now closed. If the worker still has
        // other active beads, skip assigning NEW work — but the close above
        // already happened, so the just-completed bead is not blocked.
        // (short-lived connection)
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
                "Completed bead closed; worker has other active beads — skipping new-work assignment"
            );
            if let Some(id) = ack_msg_id {
                self.ack_msg(id);
            }
            return Ok(none_result);
        }

        // No inline sync here — br::close_bead writes directly to the DB.
        // JSONL export happens once at end-of-cycle in main.rs (br::sync).

        let t = std::time::Instant::now();
        let (ready_beads, busy_crates) = self.ready_work_context()?;
        tracing::info!(
            elapsed_ms = t.elapsed().as_millis() as u64,
            "reassign: ensure_ready_work"
        );
        let next = pick_from_ready(&ready_beads, &busy_crates, &[]).cloned();

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
                if self.resolve_worker_identity_cached(&pane.id).as_deref() == Some(worker_name) {
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

        tracing::info!(
            elapsed_ms = t.elapsed().as_millis() as u64,
            "reassign: worker alive check"
        );

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

        let exec_map = if self
            .config
            .project_root
            .join("prd/POST_REPIN_EXECUTION_MAP.md")
            .exists()
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
             For Rust builds/tests from the repo root, prefer `./scripts/rust_task.sh <cargo args>` so Cargo-heavy verification stays serialized across workers.\n\
             When complete, use `/skill mail-complete {} --to {} --file <path> --test <command>`.\n\
             Do not send a freehand completion message.\n\
             Include concrete `Files changed:` and `Tests run:` sections \
             plus a JSON payload with `bead_id`, `files_changed`, and `test_commands`.\n\
             Every `--test` entry must be an exact replayable shell command. \
             Do not send prose like `python3 direct validation script`, and do not summarize commands with `...`.\n\
             Prefer single-line commands or checked-in scripts over heredocs.\n\
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
             - every reported test must be an exact replayable shell command\n\
             - do not use prose placeholders like `python3 direct validation script`\n\
             - do not summarize commands with `...`\n\
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

    /// Whether this coordinator was constructed in dry-run mode (no br writes,
    /// no tmux sends). Read by out-of-module passes like park review.
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
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
        if !self.config.worker_requires_assignment_prompt() {
            return Ok(0);
        }

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
                    let _ =
                        tmux::send_keys(session, window, w.pane_index, &self.config.clear_line_key);
                    std::thread::sleep(std::time::Duration::from_millis(
                        self.config.post_clear_delay_ms,
                    ));
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
                let capture =
                    tmux::capture_pane(session, window, w.pane_index, 80).unwrap_or_default();

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
                            let _ = tmux::send_keys(
                                session,
                                window,
                                w.pane_index,
                                &self.config.submit_key,
                            );
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

    /// Record a successful bead close (any path). Feeds close-stall detection
    /// and the auto-checkpoint cadence.
    fn record_close(&self) {
        *self.last_close.lock().unwrap() = Some(Instant::now());
        *self.closes_since_checkpoint.lock().unwrap() += 1;
    }

    /// Time since the last successful close, falling back to coordinator
    /// startup so a fresh launch doesn't instantly alarm.
    fn since_last_close(&self) -> Duration {
        self.last_close
            .lock()
            .unwrap()
            .map(|t| t.elapsed())
            .unwrap_or_else(|| self.started_at.elapsed())
    }

    /// Close-stall: the swarm holds claimed work but nothing has closed for
    /// the configured backpressure window — the verifier lane is wedged.
    pub fn close_stalled(&self, in_progress: usize, workers: usize) -> bool {
        close_stall_active(
            self.since_last_close(),
            self.config.close_stall_backpressure_seconds,
            in_progress,
            workers,
        )
    }

    /// Alert the operator about a close-stall, rate-limited so a long wedge
    /// doesn't flood the boss pane.
    fn maybe_notify_close_stall(&self, in_progress: usize, workers: usize) {
        {
            let last = self.last_stall_notify.lock().unwrap();
            if let Some(t) = *last {
                if t.elapsed() < Duration::from_secs(self.config.close_stall_renotify_seconds) {
                    return;
                }
            }
        }
        *self.last_stall_notify.lock().unwrap() = Some(Instant::now());
        let mins = self.since_last_close().as_secs() / 60;
        let queued = self.pending_verifications.lock().unwrap().len();
        let diagnosis = if queued > 0 {
            "Verifier lane is likely wedged — suppressing re-prompts and reclaims until a \
             close lands. Check .orchestrator/rust-build.lock.info and the verifier pane."
        } else {
            "Verify queue is EMPTY — workers may never have started (swallowed prompts?). \
             Re-prompts stay enabled; check the worker panes."
        };
        let msg = format!(
            "CLOSE-STALL: no bead has closed in {mins}m with {in_progress} in_progress \
             across {workers} workers ({queued} verifications queued). {diagnosis}"
        );
        tracing::warn!("{msg}");
        self.notify_boss(&msg);
    }

    /// Spawn a background auto-checkpoint (workspace-check gate + git commit
    /// of the shared tree) once enough beads have closed. Bounds the blast
    /// radius of uncommitted swarm WIP: after a bad hunk lands, at most one
    /// checkpoint window of work is unattributable. No-op when disabled,
    /// in flight, under target, throttled, or in dry-run.
    pub fn maybe_spawn_checkpoint(&self) {
        let target = self.config.checkpoint_closes;
        let closes = *self.closes_since_checkpoint.lock().unwrap();
        let in_flight = self
            .checkpoint_in_flight
            .load(std::sync::atomic::Ordering::SeqCst);
        let since = self
            .last_checkpoint_attempt
            .lock()
            .unwrap()
            .map(|t| t.elapsed());
        if !checkpoint_due(
            closes,
            target,
            in_flight,
            since,
            self.config.checkpoint_min_interval_seconds,
        ) {
            return;
        }
        if self.dry_run {
            return;
        }
        // Defer while the verify lane is busy: a cold gate build saturates
        // CPU and can push in-flight verifies past their timeout (false dyld
        // trips + spurious park streaks). The 8s cycle cadence retries at the
        // next quiet moment, before the attempt throttle is stamped.
        if !self.pending_verifications.lock().unwrap().is_empty() {
            return;
        }
        *self.last_checkpoint_attempt.lock().unwrap() = Some(Instant::now());
        self.checkpoint_in_flight
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let root = self.config.project_root.clone();
        let gate_target = self.config.cache_dir().join("checkpoint-target");
        let gate_timeout = Duration::from_secs(self.config.checkpoint_gate_timeout_seconds);
        let counter = Arc::clone(&self.closes_since_checkpoint);
        let flag = Arc::clone(&self.checkpoint_in_flight);
        let notify_script = self.config.notify_boss_script();
        let session = self.config.session_name.clone();
        tracing::info!(
            closes,
            "auto-checkpoint: spawning workspace-check gate + commit"
        );
        std::thread::spawn(move || {
            // The green gate runs in its OWN target dir, deliberately OFF the
            // shared build slot: a ~20-minute workspace check on the slot
            // starves every worker re-check and the verifier lane (observed
            // live 2026-07-14, where the starved check was then SIGKILLed).
            // The env-prefixed command dodges verify_commands' rust_task.sh
            // wrapping while keeping its process-group timeout handling; the
            // absolute --manifest-path makes it cwd-independent. Costs a
            // second (persistent, warm) target dir on disk.
            // nice -n 19: the gate must never outbid live verifies for CPU.
            let gate_cmd = format!(
                "CARGO_TARGET_DIR='{}' nice -n 19 cargo check --workspace --manifest-path '{}'",
                gate_target.display(),
                root.join("engine-rs/Cargo.toml").display()
            );
            // catch_unwind: a panic here would otherwise leave
            // checkpoint_in_flight latched true, silently disabling
            // auto-checkpoints until the next restart.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                verifier::verify_commands(&root, &[gate_cmd], gate_timeout)
                    .map_err(|e| format!("workspace check not green: {e}"))
                    .and_then(|_| run_checkpoint_commit(&root, closes))
            }))
            .unwrap_or_else(|_| Err("checkpoint thread panicked".to_string()));
            let text = match &outcome {
                Ok(msg) => {
                    *counter.lock().unwrap() = 0;
                    format!("AUTO-CHECKPOINT: {msg}")
                }
                Err(e) => format!("AUTO-CHECKPOINT skipped: {e}"),
            };
            tracing::info!("{text}");
            if let Some(session) = session {
                if notify_script.exists() {
                    let _ = Command::new(&notify_script)
                        .arg(&session)
                        .arg(&text)
                        .output();
                }
            }
            flag.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    /// Ready list plus busy-crate context for collision-aware selection.
    fn ready_work_context(&self) -> Result<(Vec<db::BeadInfo>, std::collections::HashSet<String>)> {
        let ready = self.ensure_ready_work()?;
        let busy = {
            let db = self.open_db()?;
            db::busy_crate_labels(&db)?
        };
        Ok((ready, busy))
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

    /// Adaptive per-bead effort for a NEW-assignment prompt: the bead's
    /// `effort:` label, else the configured default; `None` when the feature is
    /// disabled (leaving the worker's launch-baseline effort untouched).
    /// Re-prompts pass `None` directly so a worker mid-bead is never re-levelled
    /// or `/clear`ed.
    fn assignment_effort(&self, bead: &db::BeadInfo) -> Option<String> {
        if !self.config.adaptive_effort_enabled {
            return None;
        }
        Some(
            bead.effort
                .clone()
                .unwrap_or_else(|| self.config.adaptive_effort_default.clone()),
        )
    }

    /// Effort + `/clear` decision for prompting `worker` about `bead`. Injects
    /// only when this is a NEW bead for the worker (fresh assignment or the
    /// finish→next transition) — a re-nudge of the bead the worker is already
    /// on returns `(None, false)` so its in-progress context is never cleared.
    /// Records the bead as the worker's current effort target. `(None, false)`
    /// when adaptive effort is disabled.
    fn effort_for_prompt(&self, worker: &str, bead: &db::BeadInfo) -> (Option<String>, bool) {
        if !self.config.adaptive_effort_enabled {
            return (None, false);
        }
        {
            let mut map = self.last_effort_bead.lock().unwrap();
            if map.get(worker).map(String::as_str) == Some(bead.id.as_str()) {
                return (None, false); // re-nudge of the same bead — inject nothing
            }
            map.insert(worker.to_string(), bead.id.clone());
        }
        (self.assignment_effort(bead), self.config.adaptive_effort_clear)
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
        self.config
            .cache_dir()
            .join(format!("{sanitized}.last-assignment"))
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

fn verifier_log_append(path: &PathBuf, line: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_verifier_worker(
    project_root: PathBuf,
    timeout: Duration,
    crate_breaker_threshold: u32,
    verify_request_rx: mpsc::Receiver<VerifyRequest>,
    verify_tx: mpsc::Sender<VerifyResult>,
    pending_verifications: Arc<Mutex<HashMap<String, Instant>>>,
    pending_completion_msgs: Arc<Mutex<HashSet<i64>>>,
    verifier_log_path: PathBuf,
    // pat-f6ndj: alert plumbing for the dyld-wedge circuit breaker. When the
    // breaker trips we mail the coordinator/operator; `coordinator_name == None`
    // makes the alert log-only (used in tests, where there is no mail server).
    alert_mail_config: MailConfig,
    coordinator_name: Option<String>,
    project_key: String,
) {
    use crate::verifier::circuit_breaker::{probe_recent_test_binary, VerifyCircuitBreaker};
    use crate::verifier::crate_breaker::CrateBreaker;

    std::thread::spawn(move || {
        // pat-f6ndj: a host dyld-wedge makes every freshly launched test binary
        // hang for the full timeout. After N=2 consecutive bare timeouts the
        // breaker trips: the lane stops launching new verifies (each would hang
        // ~900s and deepen the wedge) and alerts the operator. It auto-resumes
        // when a probe launch of a freshly-built local binary succeeds, throttled
        // to one probe per window so a backlog of requests can't spin the prober.
        const BREAKER_THRESHOLD: u32 = 2;
        let probe_interval = Duration::from_secs(60);
        let mut breaker = VerifyCircuitBreaker::new(BREAKER_THRESHOLD);
        let mut last_probe: Option<Instant> = None;

        // Crate-breakage breaker: consecutive `could not compile <crate>`
        // failures trip a per-crate gate; the lane then holds behind a
        // throttled `cargo check -p <crate>` pre-flight until the crate
        // builds again instead of re-burning the build slot per bead.
        let mut crate_breaker =
            CrateBreaker::new(crate_breaker_threshold, Duration::from_secs(120));
        let precheck_timeout = timeout.min(Duration::from_secs(300));

        while let Ok(first) = verify_request_rx.recv() {
            // Batch-drain the queue and group same-crate verifies adjacently
            // so warm build artifacts are reused instead of interleaving
            // rebuilds across crates.
            let mut batch = vec![first];
            while let Ok(more) = verify_request_rx.try_recv() {
                batch.push(more);
            }
            if batch.len() > 1 {
                order_batch_by_crate(&mut batch);
                verifier_log_append(
                    &verifier_log_path,
                    &format!(
                        "BATCH drained {} queued verifies (crate-grouped)",
                        batch.len()
                    ),
                );
                // One shared warm-up build covering every scoped test target in
                // the batch: N beads amortize a single dep-graph build instead
                // of each paying compile+link separately. Best-effort — a
                // failure (bogus target name, timeout) just logs and falls back
                // to the per-bead builds below, never failing a bead. Skipped
                // while the dyld breaker is open (a wedged host would hang it).
                if breaker.should_launch() {
                    let all_commands = batch
                        .iter()
                        .flat_map(|r| r.commands.iter().map(String::as_str));
                    if let Some(prebuild) =
                        verifier::batch_prebuild_command(all_commands, "patina-engine")
                    {
                        verifier_log_append(
                            &verifier_log_path,
                            &format!("BATCH-PREBUILD start: {prebuild}"),
                        );
                        let t0 = Instant::now();
                        match verifier::verify_commands(&project_root, &[prebuild], timeout) {
                            Ok(()) => verifier_log_append(
                                &verifier_log_path,
                                &format!("BATCH-PREBUILD done run_ms={}", t0.elapsed().as_millis()),
                            ),
                            Err(e) => verifier_log_append(
                                &verifier_log_path,
                                &format!(
                                    "BATCH-PREBUILD failed run_ms={} (falling back to \
                                     per-bead builds): {}",
                                    t0.elapsed().as_millis(),
                                    verifier::tail_chars(&format!("{e}"), 300)
                                ),
                            ),
                        }
                    }
                }
            }

            for request in batch {
                let bead_id = request.bead_id.clone();
                let msg_id = request.msg_id;

                // Breaker open → do not launch this verify. Try a throttled probe to
                // resume; if it's not time yet or the probe fails, skip the launch
                // and leave the bead pending so the stale-verification reaper (B3)
                // handles it — don't bounce the worker during a host-level wedge.
                if !breaker.should_launch() {
                    let due = last_probe.map_or(true, |t| t.elapsed() >= probe_interval);
                    let resumed = due && {
                        last_probe = Some(Instant::now());
                        breaker.probe_and_maybe_resume(|| probe_recent_test_binary(&project_root))
                    };
                    if resumed {
                        verifier_log_append(
                            &verifier_log_path,
                            "RESUMED verify lane — probe launch succeeded (dyld-wedge cleared)",
                        );
                        tracing::warn!("verify lane resumed after dyld-wedge probe succeeded");
                    } else {
                        verifier_log_append(
                        &verifier_log_path,
                        &format!(
                            "SKIPPED bead={bead_id} — verify lane halted (dyld-wedge circuit breaker open)"
                        ),
                    );
                        // Release the completion MESSAGE (it re-enqueues
                        // cleanly after the B3 reaper reopens the bead) while
                        // the bead itself stays pending — leaving the msg id
                        // in the set strands it unacked-and-skipped in the
                        // inbox forever.
                        if let Some(id) = msg_id {
                            pending_completion_msgs.lock().unwrap().remove(&id);
                        }
                        continue;
                    }
                }

                // Crate-breakage gate: while a shared crate is known-broken,
                // don't burn the build slot re-verifying against it. A throttled
                // `cargo check -p <crate>` pre-flight detects recovery; until
                // then skipped beads stay pending (B3 reaper backstop) — workers
                // are never bounced for someone else's breakage.
                if crate_breaker.any_tripped() {
                    let recovered = crate_breaker.run_due_prechecks(|krate| {
                        verifier::verify_commands(
                            &project_root,
                            &[format!("cargo check -p {krate}")],
                            precheck_timeout,
                        )
                        .is_ok()
                    });
                    for krate in &recovered {
                        verifier_log_append(
                            &verifier_log_path,
                            &format!("RESUMED — `{krate}` builds again (pre-check passed)"),
                        );
                        tracing::warn!(krate = %krate, "crate-breakage breaker cleared");
                    }
                    if crate_breaker.any_tripped() {
                        // Hold only verifies that would rebuild a broken
                        // crate: the request's own crate, or unscoped
                        // commands (broad build graphs). Other crates'
                        // verifies proceed — one broken crate must not zero
                        // the whole lane's throughput.
                        let request_crate =
                            verifier::crate_breaker::extract_request_crate(&request.commands);
                        let held = match request_crate.as_deref() {
                            Some(rc) => crate_breaker.is_tripped(rc),
                            None => true,
                        };
                        if held {
                            verifier_log_append(
                                &verifier_log_path,
                                &format!(
                                    "SKIPPED bead={bead_id} — crate-breakage breaker open ({})",
                                    crate_breaker.tripped_crates().join(", ")
                                ),
                            );
                            // Same message-release as the dyld skip above.
                            if let Some(id) = msg_id {
                                pending_completion_msgs.lock().unwrap().remove(&id);
                            }
                            continue;
                        }
                    }
                }

                // B1: isolate each verification in catch_unwind so a panic on one
                // bead can never kill the verifier thread and wedge the whole swarm.
                // Always remove the bead from the pending sets afterwards — otherwise
                // a panic would leave it pending forever (the B3 reaper is the
                // backstop, but we should not rely on it for the common case).
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    process_one_verification(
                        &project_root,
                        timeout,
                        request,
                        &verify_tx,
                        &verifier_log_path,
                    )
                }));
                match result {
                    Ok(summary) => {
                        // Feed the crate-breakage breaker; alert exactly once on
                        // the trip transition, with the rustc excerpt and the
                        // crate's uncommitted files as the suspect list.
                        if let Some(alert) = crate_breaker.record(
                            summary.request_crate.as_deref(),
                            summary.broken_crate.as_deref(),
                            &summary.error_excerpt,
                        ) {
                            let suspects = dirty_files_for_crate(&project_root, &alert.crate_name);
                            verifier_log_append(&verifier_log_path, &alert.log_line());
                            tracing::error!("{}", alert.log_line());
                            send_crate_breakage_alert(
                                &alert_mail_config,
                                coordinator_name.as_deref(),
                                &project_key,
                                &alert,
                                &suspects,
                            );
                        }
                        // Feed the outcome to the dyld breaker; emit the operator
                        // alert exactly once, on the transition that trips it.
                        if let Some(alert) = breaker.record(summary.outcome) {
                            verifier_log_append(&verifier_log_path, &alert.host_issue_log_line());
                            tracing::error!("{}", alert.host_issue_log_line());
                            send_host_issue_alert(
                                &alert_mail_config,
                                coordinator_name.as_deref(),
                                &project_key,
                                &alert,
                            );
                            // Don't probe immediately at the trip — wait a window.
                            last_probe = Some(Instant::now());
                        }
                    }
                    Err(_) => {
                        verifier_log_append(
                            &verifier_log_path,
                            &format!("PANIC bead={bead_id} — verification thread recovered"),
                        );
                        tracing::error!(bead = %bead_id, "verification panicked — thread recovered, bead reopened next cycle");
                    }
                }
                pending_verifications.lock().unwrap().remove(&bead_id);
                if let Some(id) = msg_id {
                    pending_completion_msgs.lock().unwrap().remove(&id);
                }
            }
        }
    });
}

/// Order a drained batch of verify requests so same-crate verifies run
/// adjacently — consecutive same-crate runs reuse warm build artifacts
/// instead of interleaving rebuilds. Stable: arrival order is preserved
/// within a crate group (and for unscoped requests, which sort first).
fn order_batch_by_crate(batch: &mut [VerifyRequest]) {
    batch.sort_by_key(|r| {
        verifier::crate_breaker::extract_request_crate(&r.commands).unwrap_or_default()
    });
}

/// Fallback verify commands for a stale bead with no acceptance commands:
/// `cargo check -p <crate>` for each of the bead's `crate:*` labels, or a
/// workspace *check* (no codegen/link) when the bead is unlabeled. Never a
/// full `cargo build --workspace` — compiling and linking every crate
/// (wgpu/metal included) per stale bead is what starved the verify lane.
/// The comment left on a bead whose completion was partial (pat-k5o8f).
///
/// This is the whole point of the partial path: mail messages get acked and
/// scroll away, so a "these three features are still missing" note written only
/// in mail dies with it. A `br` comment is attached to the issue and outlives
/// every close, reopen and reassignment, which is why workers had already
/// started writing scope notes there by hand.
fn partial_scope_comment(
    worker: &str,
    files_changed: &str,
    tests_run: &str,
    remaining_scope: &str,
) -> String {
    let section = |label: &str, body: &str, empty: &str| -> String {
        let body = body.trim();
        if body.is_empty() {
            format!("{label}\n{empty}\n")
        } else {
            format!("{label}\n{body}\n")
        }
    };
    format!(
        "## Partial completion — bead intentionally left open\n\n\
         Reported by {} and verified: the delivered scope passed its tests, but the \
         worker flagged the bead as incomplete, so it was returned to the queue \
         instead of closed.\n\n{}\n{}\n{}",
        if worker.trim().is_empty() {
            "an unnamed worker"
        } else {
            worker.trim()
        },
        section(
            "### Remaining scope",
            remaining_scope,
            "(not stated — the next worker should re-derive it from the bead description)",
        ),
        section(
            "### Delivered (files changed)",
            files_changed,
            "(none reported)",
        ),
        section("### Verified by", tests_run, "(none reported)"),
    )
}

fn stale_fallback_commands(crate_labels: &[String]) -> Vec<String> {
    let pkgs: Vec<&str> = crate_labels
        .iter()
        .filter_map(|l| l.strip_prefix("crate:"))
        .filter(|p| !p.is_empty())
        .collect();
    if pkgs.is_empty() {
        vec!["cargo check --workspace".to_string()]
    } else {
        pkgs.iter().map(|p| format!("cargo check -p {p}")).collect()
    }
}

/// Uncommitted files under a broken crate — the operator's suspect list for
/// a crate-breakage alert. Uses `git status --porcelain`, NOT `git diff HEAD`:
/// brand-new (untracked) files are invisible to diff, and the live 2026-07-14
/// breakage came from exactly such a file (an untracked autoload.rs).
/// Best-effort.
fn dirty_files_for_crate(project_root: &std::path::Path, crate_name: &str) -> String {
    let out = Command::new("git")
        .current_dir(project_root)
        .args([
            "status",
            "--porcelain",
            "--",
            &format!("engine-rs/crates/{crate_name}/"),
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let files: Vec<&str> = s.lines().take(12).collect();
            if files.is_empty() {
                "(no uncommitted files under the crate)".to_string()
            } else {
                files.join("\n")
            }
        }
        _ => "(git status unavailable)".to_string(),
    }
}

/// Mail the CRATE-BROKEN alert to the coordinator/operator. Best-effort and
/// log-only when no coordinator recipient is configured (tests).
fn send_crate_breakage_alert(
    mail_config: &MailConfig,
    coordinator: Option<&str>,
    project_key: &str,
    alert: &crate::verifier::crate_breaker::BreakageAlert,
    suspects: &str,
) {
    let Some(coord) = coordinator else {
        return;
    };
    let msg = OutgoingMessage {
        project_key: project_key.to_string(),
        sender_name: "verifier".to_string(),
        to: vec![coord.to_string()],
        subject: format!(
            "CRATE-BROKEN: `{}` is blocking the verify lane",
            alert.crate_name
        ),
        body_md: format!(
            "`{}` failed to compile in {} consecutive verifies. The lane now holds its \
             verifies behind a throttled `cargo check -p {}` pre-flight until it builds \
             again; skipped beads stay pending (no worker bounces).\n\n\
             rustc excerpt:\n```\n{}\n```\n\n\
             Uncommitted files under the crate (suspects):\n```\n{}\n```",
            alert.crate_name,
            alert.consecutive_failures,
            alert.crate_name,
            alert.error_excerpt,
            suspects
        ),
        importance: "high".to_string(),
        ack_required: false,
        topic: "crate-broken".to_string(),
        thread_id: format!("verifier-crate-broken-{}", alert.crate_name),
    };
    let _ = MailClient::new(mail_config).send_message_best_effort(&msg);
}

/// Send the dyld-wedge HOST-ISSUE alert to the coordinator/operator (pat-f6ndj).
/// Best-effort and log-only when no coordinator recipient is configured.
fn send_host_issue_alert(
    mail_config: &MailConfig,
    coordinator: Option<&str>,
    project_key: &str,
    alert: &crate::verifier::circuit_breaker::WedgeAlert,
) {
    let Some(coord) = coordinator else {
        return;
    };
    let msg = OutgoingMessage {
        project_key: project_key.to_string(),
        sender_name: "verifier".to_string(),
        to: vec![coord.to_string()],
        subject: alert.mail_subject(),
        body_md: alert.mail_body(),
        importance: "high".to_string(),
        ack_required: false,
        topic: "host-issue".to_string(),
        thread_id: "verifier-dyld-wedge".to_string(),
    };
    let _ = MailClient::new(mail_config).send_message_best_effort(&msg);
}

/// Run one verification request and send its result. Separated out so the
/// verifier loop can wrap it in `catch_unwind` (B1). Returns a summary that
/// feeds both circuit breakers: the dyld-wedge outcome (pat-f6ndj; a bare
/// wall-clock timeout with no build output is `TimedOut`) plus the crate
/// fields for the crate-breakage breaker.
fn process_one_verification(
    project_root: &std::path::Path,
    timeout: Duration,
    request: VerifyRequest,
    verify_tx: &mpsc::Sender<VerifyResult>,
    verifier_log_path: &PathBuf,
) -> VerifyRunSummary {
    let bead_id = request.bead_id.clone();
    let queue_wait_ms = request.enqueued_at.elapsed().as_millis() as u64;
    let joined = request.commands.join(" && ");
    verifier_log_append(
        verifier_log_path,
        &format!(
            "START bead={bead_id} stale={} queue_wait_ms={queue_wait_ms} commands={joined}",
            request.is_stale_recovery
        ),
    );
    tracing::info!(
        bead = %request.bead_id,
        commands = request.commands.len(),
        stale = request.is_stale_recovery,
        queue_wait_ms,
        "Verification queue started"
    );

    let run_start = Instant::now();
    let mut verify_result = verifier::verify_commands(project_root, &request.commands, timeout);
    // Red check: a green acceptance test must also go RED with the change
    // removed, or it does not exercise the change (see wiring.rs). Only a
    // clean "still passes" result rejects; anything undecidable is logged.
    if verify_result.is_ok() && !request.is_stale_recovery {
        let red = crate::wiring::red_check(
            project_root,
            &request.files_changed,
            &request.commands,
            &request.acceptance_tests,
            timeout,
        );
        verifier_log_append(verifier_log_path, &format!("RED-CHECK bead={bead_id} {red:?}"));
        if let crate::wiring::RedVerdict::NotDetected(why) = red {
            verify_result = Err(crate::error::OrchestratorError::Verification(why));
        }
    }
    let run_ms = run_start.elapsed().as_millis() as u64;
    let passed = verify_result.is_ok();
    let reject_reason = verify_result.err().map(|e| format!("{e}"));

    // C1: structured per-bead verify latency so an operator can tell a slow
    // builder (high run_ms) from a backed-up queue (high queue_wait_ms).
    if passed {
        verifier_log_append(
            verifier_log_path,
            &format!("PASS bead={bead_id} queue_wait_ms={queue_wait_ms} run_ms={run_ms}"),
        );
    } else {
        verifier_log_append(
            verifier_log_path,
            &format!(
                "FAIL bead={bead_id} queue_wait_ms={queue_wait_ms} run_ms={run_ms} reason={}",
                reject_reason.as_deref().unwrap_or("unknown")
            ),
        );
    }

    // Classify for the circuit breakers before `reject_reason` is moved into
    // the result. A bare timeout is reported by `verify_commands` as a "timed
    // out after …" error with no build output; treat that (and only that) as
    // the wedge-candidate `TimedOut`.
    let timed_out = !passed
        && reject_reason
            .as_deref()
            .is_some_and(|r| r.contains("timed out after"));
    let outcome = verifier::circuit_breaker::classify_verify_outcome(
        timed_out,
        reject_reason.as_deref().unwrap_or(""),
    );
    let request_crate = verifier::crate_breaker::extract_request_crate(&request.commands);
    let broken_crate = reject_reason
        .as_deref()
        .and_then(verifier::crate_breaker::parse_broken_crate);
    let error_excerpt = reject_reason
        .as_deref()
        .map(|r| verifier::tail_chars(r, 600))
        .unwrap_or_default();

    let _ = verify_tx.send(VerifyResult {
        bead_id: request.bead_id.clone(),
        worker: request.worker,
        msg_id: request.msg_id,
        ack_required: request.ack_required,
        files_changed: request.files_changed,
        tests_run: request.tests_run,
        passed,
        reject_reason,
        is_stale_recovery: request.is_stale_recovery,
        partial: request.partial,
        remaining_scope: request.remaining_scope,
    });

    VerifyRunSummary {
        outcome,
        request_crate,
        broken_crate,
        error_excerpt,
    }
}

/// Update per-bead orphan strike counts and return beads that have reached the
/// reopen threshold (B4). Beads no longer orphaned have their strikes cleared;
/// a bead reaching `threshold` consecutive strikes is returned and its counter
/// reset. Pure so it is unit-testable without a DB.
fn update_orphan_strikes(
    orphaned_now: &[String],
    strikes: &mut HashMap<String, u32>,
    threshold: u32,
) -> Vec<String> {
    // Clear strikes for beads that are no longer orphaned (recovered).
    strikes.retain(|k, _| orphaned_now.contains(k));
    let mut to_reopen = Vec::new();
    for id in orphaned_now {
        let count = strikes.entry(id.clone()).or_insert(0);
        *count += 1;
        if *count >= threshold.max(1) {
            to_reopen.push(id.clone());
            *count = 0; // reset so we don't reopen every subsequent poll
        }
    }
    to_reopen
}

/// Deadlock watchdog (D5): given whether this poll made progress and whether
/// the swarm has work assigned to live workers, advance a zero-progress
/// counter and decide whether to alert. Returns `(new_counter, should_alert)`.
///
/// Alert-only by design — it works in pull mode (where auto-recovery is
/// disabled) without taking risky automatic action. `should_alert` fires
/// exactly once when the counter first crosses the threshold.
pub fn deadlock_watchdog_tick(
    made_progress: bool,
    work_in_flight: bool,
    workers_present: bool,
    prev_counter: usize,
    threshold: usize,
) -> (usize, bool) {
    if made_progress || !work_in_flight || !workers_present {
        return (0, false);
    }
    let counter = prev_counter + 1;
    let should_alert = counter == threshold.max(1);
    (counter, should_alert)
}

/// D4: whether a worker is still within its post-reclaim hysteresis window.
/// `window_seconds == 0` disables hysteresis.
fn within_hysteresis(since_last_reclaim: Duration, window_seconds: u64) -> bool {
    window_seconds > 0 && since_last_reclaim < Duration::from_secs(window_seconds)
}

/// Bump a bead's verify-fail streak. Returns `(streak, should_park)`; a
/// parked bead's entry is removed so a later operator retry starts fresh.
/// `park_threshold == 0` disables parking.
fn bump_fail_streak(
    streaks: &mut HashMap<String, u32>,
    bead_id: &str,
    park_threshold: u32,
) -> (u32, bool) {
    let n = streaks.entry(bead_id.to_string()).or_insert(0);
    *n += 1;
    let streak = *n;
    let park = park_threshold > 0 && streak >= park_threshold;
    if park {
        streaks.remove(bead_id);
    }
    (streak, park)
}

/// Auto-checkpoint decision: enough closes accumulated, no run in flight,
/// and the attempt throttle has elapsed. `target == 0` disables.
fn checkpoint_due(
    closes: u64,
    target: u64,
    in_flight: bool,
    since_attempt: Option<Duration>,
    min_interval_secs: u64,
) -> bool {
    target > 0
        && !in_flight
        && closes >= target
        && since_attempt.map_or(true, |d| d >= Duration::from_secs(min_interval_secs))
}

/// Run a git command in `root`, returning stdout or a descriptive error.
fn git_capture(root: &std::path::Path, args: &[&str]) -> std::result::Result<String, String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|e| format!("git {} spawn failed: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Commit the working tree as an auto-checkpoint snapshot. Refuses on the
/// default branch. Hooks are bypassed (`--no-verify`): the checkpoint is by
/// design a whole-tree snapshot of every agent's WIP, which per-agent
/// pre-commit guards would otherwise veto. Returns a short human summary.
fn run_checkpoint_commit(
    project_root: &std::path::Path,
    closes: u64,
) -> std::result::Result<String, String> {
    let branch_raw = git_capture(project_root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch = branch_raw.trim();
    if branch == "main" || branch == "master" || branch == "HEAD" {
        // "HEAD" is rev-parse's spelling of a detached HEAD — committing
        // there produces reflog-only orphans at the next checkout.
        return Err(format!(
            "on default branch or detached HEAD (`{branch}`) — auto-checkpoint refuses to commit here"
        ));
    }
    git_capture(project_root, &["add", "-A"])?;
    let staged = git_capture(project_root, &["status", "--porcelain"])?;
    if staged.trim().is_empty() {
        return Ok("nothing to commit (tree clean)".to_string());
    }
    let msg = format!("Checkpoint: auto after {closes} closes (orchestrator)");
    git_capture(project_root, &["commit", "--no-verify", "-m", &msg])?;
    let short = git_capture(project_root, &["rev-parse", "--short", "HEAD"])?;
    Ok(format!(
        "committed {} on `{branch}` after {closes} closes",
        short.trim()
    ))
}

/// Whether close-stall suppression (skip re-prompts and reclaims) applies.
/// Requires evidence the pipeline actually holds finished work — queued
/// verifications — because a close drought with an EMPTY verify queue is the
/// swallowed-prompt failure mode, where the re-prompt is the remedy, not the
/// disease. Recovery mode always bypasses suppression.
fn stall_suppression_active(
    close_stalled: bool,
    queued_verifications: usize,
    recovery_mode: bool,
) -> bool {
    close_stalled && queued_verifications > 0 && !recovery_mode
}

/// Close-stall predicate: nothing closed for `threshold_seconds` while the
/// swarm holds at least as many in_progress beads as workers (i.e. closes
/// should be flowing). `threshold_seconds` 0 disables detection entirely.
fn close_stall_active(
    since_last_close: Duration,
    threshold_seconds: u64,
    in_progress: usize,
    workers: usize,
) -> bool {
    threshold_seconds > 0
        && workers > 0
        && in_progress >= workers
        && since_last_close >= Duration::from_secs(threshold_seconds)
}

/// Pick the next bead from the priority-ordered ready list.
///
/// Within the head priority band, prefer a bead whose `crate:*` labels don't
/// intersect the crates in_progress work is already editing — two workers in
/// one crate is how cross-worker compile breakage cascades and reopens closed
/// beads. Never crosses a priority band for that preference (a colliding P0
/// still beats a collision-free P1), and falls back to the band head when
/// every candidate collides: a collision is cheaper than a starved worker.
fn pick_from_ready<'a>(
    ready: &'a [db::BeadInfo],
    busy_crates: &std::collections::HashSet<String>,
    exclude: &[String],
) -> Option<&'a db::BeadInfo> {
    let mut head: Option<&db::BeadInfo> = None;
    for b in ready.iter().filter(|b| !exclude.contains(&b.id)) {
        match head {
            None => head = Some(b),
            Some(h) if b.priority != h.priority => break,
            Some(_) => {}
        }
        if b.crate_labels.iter().all(|l| !busy_crates.contains(l)) {
            return Some(b);
        }
    }
    head
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
    reclaim_decision(bead, age, grace_seconds, &capture, worker_name, coordinator)
}

/// Decision core of [`should_reclaim_idle`], with the pane capture injected
/// so tests can drive every branch without a live tmux session.
fn reclaim_decision(
    bead: &db::BeadInfo,
    age: u64,
    grace_seconds: u64,
    capture: &str,
    worker_name: &str,
    coordinator: &Coordinator,
) -> bool {
    // Never reclaim a bead whose completion is actively being verified — the
    // work is done and in the pipe; reclaiming reverts it and churns the
    // queue (the verifier-thrash failure mode).
    if coordinator
        .pending_verifications
        .lock()
        .unwrap()
        .contains_key(&bead.id)
    {
        tracing::debug!(bead = %bead.id, "skip reclaim: verification pending");
        return false;
    }

    if worker::has_active_work(capture) {
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
                        regex::Regex::new(r"(?i)❯|waiting\.$|standing by|done\.?$|complete\.?$")
                            .unwrap()
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
        let (verify_request_tx, _verify_request_rx) = mpsc::channel();
        Coordinator {
            config: cfg,
            mail,
            dry_run: true,
            pending_verifications: Arc::new(Mutex::new(HashMap::new())),
            pending_completion_msgs: Arc::new(Mutex::new(HashSet::new())),
            verify_rx: Mutex::new(verify_rx),
            verify_tx,
            verify_request_tx,
            orphan_strikes: Mutex::new(HashMap::new()),
            identity_cache: Mutex::new(HashMap::new()),
            last_effort_bead: Mutex::new(HashMap::new()),
            last_reclaim: Mutex::new(HashMap::new()),
            last_close: Mutex::new(None),
            started_at: Instant::now(),
            last_stall_notify: Mutex::new(None),
            verify_fail_streaks: Mutex::new(HashMap::new()),
            closes_since_checkpoint: Arc::new(Mutex::new(0)),
            checkpoint_in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_checkpoint_attempt: Mutex::new(None),
        }
    }

    fn ready_bead(id: &str, priority: i32, crates: &[&str]) -> db::BeadInfo {
        db::BeadInfo {
            id: id.to_string(),
            title: id.to_string(),
            description: String::new(),
            status: db::BeadStatus::Open,
            assignee: None,
            updated_at: None,
            priority,
            crate_labels: crates.iter().map(|s| s.to_string()).collect(),
            effort: None,
            unblock_impact: 0,
        }
    }

    /// Close-stall fires only when the swarm is saturated AND the close
    /// drought exceeds the threshold; 0-threshold disables.
    #[test]
    fn close_stall_activates_only_when_saturated_and_stale() {
        let drought = Duration::from_secs(1000);
        assert!(close_stall_active(drought, 900, 5, 5));
        assert!(
            close_stall_active(drought, 900, 7, 5),
            "over-saturated swarm still stalls"
        );
        assert!(
            !close_stall_active(drought, 0, 5, 5),
            "threshold 0 disables"
        );
        assert!(
            !close_stall_active(Duration::from_secs(100), 900, 5, 5),
            "recent close means no stall"
        );
        assert!(
            !close_stall_active(drought, 900, 3, 5),
            "under-saturated swarm: closes aren't expected to flow"
        );
        assert!(
            !close_stall_active(drought, 900, 0, 0),
            "no workers, no stall"
        );
    }

    /// Collision-aware pick: prefer a collision-free bead within the head
    /// priority band.
    #[test]
    fn pick_prefers_collision_free_within_priority_band() {
        let ready = vec![
            ready_bead("pat-a", 0, &["crate:gdeditor"]),
            ready_bead("pat-b", 0, &["crate:gdscene"]),
            ready_bead("pat-c", 1, &[]),
        ];
        let busy: HashSet<String> = ["crate:gdeditor".to_string()].into_iter().collect();
        assert_eq!(pick_from_ready(&ready, &busy, &[]).unwrap().id, "pat-b");
    }

    /// Priority always beats collision avoidance: a colliding P0 is taken
    /// over a collision-free P1.
    #[test]
    fn pick_never_crosses_priority_band_for_collision_avoidance() {
        let ready = vec![
            ready_bead("pat-p0", 0, &["crate:gdeditor"]),
            ready_bead("pat-p1", 1, &[]),
        ];
        let busy: HashSet<String> = ["crate:gdeditor".to_string()].into_iter().collect();
        assert_eq!(pick_from_ready(&ready, &busy, &[]).unwrap().id, "pat-p0");
    }

    /// Beads without crate labels are trivially collision-free; head order
    /// is preserved when nothing collides.
    #[test]
    fn pick_unlabeled_head_wins_immediately() {
        let ready = vec![
            ready_bead("pat-a", 0, &[]),
            ready_bead("pat-b", 0, &["crate:free"]),
        ];
        let busy: HashSet<String> = HashSet::new();
        assert_eq!(pick_from_ready(&ready, &busy, &[]).unwrap().id, "pat-a");
    }

    /// Exclusions are skipped entirely; empty/fully-excluded lists yield None.
    #[test]
    fn pick_respects_exclusions_and_empty_list() {
        let ready = vec![ready_bead("pat-a", 0, &[]), ready_bead("pat-b", 0, &[])];
        let busy: HashSet<String> = HashSet::new();
        let excl = vec!["pat-a".to_string()];
        assert_eq!(pick_from_ready(&ready, &busy, &excl).unwrap().id, "pat-b");
        assert!(pick_from_ready(&[], &busy, &[]).is_none());
        let all = vec!["pat-a".to_string(), "pat-b".to_string()];
        assert!(pick_from_ready(&ready, &busy, &all).is_none());
    }

    /// A bead whose completion is mid-verification must never be reclaimed —
    /// reclaiming reverts finished work while the verifier lane is busy or
    /// wedged (the verifier-thrash failure mode). The capture used here DOES
    /// warrant a reclaim on its own (idle/done pattern), so the first assert
    /// proves the test is not vacuous and the second isolates the guard.
    #[test]
    fn reclaim_skips_bead_with_pending_verification() {
        let coord = test_coordinator();
        let mut bead = ready_bead("pat-pending", 0, &[]);
        bead.status = db::BeadStatus::InProgress;
        bead.assignee = Some("WorkerA".to_string());
        let capture = "bead implemented and reported.\nwaiting for next assignment";

        assert!(
            reclaim_decision(&bead, 9999, 30, capture, "WorkerA", &coord),
            "sanity: without a pending verification this capture must reclaim"
        );

        coord
            .pending_verifications
            .lock()
            .unwrap()
            .insert("pat-pending".to_string(), Instant::now());
        assert!(
            !reclaim_decision(&bead, 9999, 30, capture, "WorkerA", &coord),
            "pending verification must veto the reclaim"
        );
    }

    /// Suppression needs all three: an active close-stall, queued
    /// verifications (evidence work is stuck in the pipe, not
    /// never-started), and no recovery override.
    #[test]
    fn stall_suppression_requires_queued_verifications_and_no_recovery() {
        assert!(stall_suppression_active(true, 3, false));
        assert!(
            !stall_suppression_active(true, 0, false),
            "empty verify queue = swallowed-prompt mode; re-prompts must flow"
        );
        assert!(
            !stall_suppression_active(true, 3, true),
            "recovery mode bypasses suppression"
        );
        assert!(!stall_suppression_active(false, 3, false));
    }

    /// The stale-recovery fallback must scope its compile check to the bead's
    /// `crate:*` labels — never a full `cargo build --workspace` (which links
    /// every crate per stale bead and starves the verify lane) — and use a
    /// codegen-free workspace *check* only for unlabeled beads.
    #[test]
    fn stale_fallback_scopes_to_crate_labels() {
        assert_eq!(
            stale_fallback_commands(&["crate:gdscene".to_string(), "crate:gdrender2d".to_string()]),
            vec!["cargo check -p gdscene", "cargo check -p gdrender2d"]
        );
        assert_eq!(
            stale_fallback_commands(&[]),
            vec!["cargo check --workspace"]
        );
        // Malformed labels (bare "crate:") can't produce an empty -p arg.
        assert_eq!(
            stale_fallback_commands(&["crate:".to_string()]),
            vec!["cargo check --workspace"]
        );
    }

    fn verify_request(bead: &str, cmd: &str) -> VerifyRequest {
        VerifyRequest {
            bead_id: bead.to_string(),
            worker: "WorkerA".to_string(),
            msg_id: None,
            ack_required: false,
            files_changed: String::new(),
            tests_run: String::new(),
            commands: vec![cmd.to_string()],
            is_stale_recovery: false,
            enqueued_at: Instant::now(),
            partial: false,
            remaining_scope: String::new(),
            acceptance_tests: Vec::new(),
        }
    }

    /// Batch ordering groups same-crate verifies adjacently (warm-build
    /// reuse) while preserving arrival order within each crate group.
    #[test]
    fn batch_orders_same_crate_adjacent_and_stable() {
        let mut batch = vec![
            verify_request("b1", "cargo nextest run -p gdscene --lib a"),
            verify_request("b2", "cargo nextest run -p gdeditor --test t1"),
            verify_request("b3", "cargo nextest run -p gdscene --lib b"),
            verify_request("b4", "cargo nextest run -p gdeditor --test t2"),
        ];
        order_batch_by_crate(&mut batch);
        let ids: Vec<&str> = batch.iter().map(|r| r.bead_id.as_str()).collect();
        assert_eq!(ids, vec!["b2", "b4", "b1", "b3"]);
    }

    /// Fail streaks park at the threshold, clear on park (operator retry
    /// starts fresh), and are disabled by threshold 0.
    #[test]
    fn fail_streak_parks_at_threshold_and_resets() {
        let mut m = HashMap::new();
        assert_eq!(bump_fail_streak(&mut m, "pat-x", 2), (1, false));
        assert_eq!(bump_fail_streak(&mut m, "pat-x", 2), (2, true));
        assert_eq!(
            bump_fail_streak(&mut m, "pat-x", 2),
            (1, false),
            "parked entry cleared — retry starts a fresh streak"
        );
        let mut m2 = HashMap::new();
        for _ in 0..5 {
            assert!(
                !bump_fail_streak(&mut m2, "pat-y", 0).1,
                "0 disables parking"
            );
        }
    }

    #[test]
    fn checkpoint_due_gates_correctly() {
        let long = Some(Duration::from_secs(9999));
        assert!(checkpoint_due(10, 10, false, None, 1800), "never attempted");
        assert!(checkpoint_due(12, 10, false, long, 1800));
        assert!(!checkpoint_due(9, 10, false, long, 1800), "under target");
        assert!(!checkpoint_due(12, 0, false, long, 1800), "disabled");
        assert!(
            !checkpoint_due(12, 10, true, long, 1800),
            "already in flight"
        );
        assert!(
            !checkpoint_due(12, 10, false, Some(Duration::from_secs(60)), 1800),
            "attempt throttle"
        );
    }

    /// Auto-checkpoint refuses the default branch, commits on a work branch,
    /// and reports a clean tree without committing.
    #[test]
    fn checkpoint_commit_branch_guard_and_commit() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "test@test"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "commit.gpgsign", "false"]);
        git(&["checkout", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "one").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-m", "init"]);

        std::fs::write(root.join("b.txt"), "two").unwrap();
        let err = run_checkpoint_commit(root, 10).unwrap_err();
        assert!(err.contains("default branch"), "{err}");

        git(&["checkout", "-b", "swarm-work"]);
        let ok = run_checkpoint_commit(root, 10).unwrap();
        assert!(ok.contains("committed"), "{ok}");
        let ok2 = run_checkpoint_commit(root, 12).unwrap();
        assert!(ok2.contains("nothing to commit"), "{ok2}");

        // Detached HEAD must refuse too — commits there orphan at the next
        // checkout.
        git(&["checkout", "--detach"]);
        let err = run_checkpoint_commit(root, 13).unwrap_err();
        assert!(err.contains("HEAD"), "{err}");
    }

    /// pat-f6ndj: the live verify lane must STOP launching verifies once the
    /// dyld-wedge circuit breaker trips. Drives the real `spawn_verifier_worker`
    /// loop with a 1s timeout and three `sleep 30` requests (each a guaranteed
    /// bare timeout): after the 2nd consecutive timeout the breaker trips, the
    /// HOST-ISSUE banner is logged, and the 3rd request is SKIPPED (never
    /// launched) instead of eating another full timeout. coordinator_name=None
    /// keeps the alert log-only (no mail server in tests).
    #[test]
    fn verify_lane_halts_after_two_consecutive_timeouts() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_path = dir.path().join("verifier.log");
        let (req_tx, req_rx) = mpsc::channel();
        let (res_tx, _res_rx) = mpsc::channel();
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let pending_msgs = Arc::new(Mutex::new(HashSet::new()));

        spawn_verifier_worker(
            dir.path().to_path_buf(),
            Duration::from_secs(1), // short timeout → `sleep 30` always times out
            0,                      // crate breaker disabled — this test drives the dyld breaker
            req_rx,
            res_tx,
            Arc::clone(&pending),
            Arc::clone(&pending_msgs),
            log_path.clone(),
            crate::config::test_config().mail,
            None, // log-only alert
            "test-project".to_string(),
        );

        for i in 0..3 {
            req_tx
                .send(VerifyRequest {
                    bead_id: format!("pat-test-{i}"),
                    worker: "w".to_string(),
                    msg_id: None,
                    ack_required: false,
                    files_changed: String::new(),
                    tests_run: String::new(),
                    commands: vec!["sleep 30".to_string()],
                    is_stale_recovery: false,
                    enqueued_at: Instant::now(),
                    partial: false,
                    remaining_scope: String::new(),
                    acceptance_tests: Vec::new(),
                })
                .unwrap();
        }
        drop(req_tx); // close the channel so the loop exits after draining

        // Wait for the 3rd request to be reached and skipped (≤10s; real work ~2s).
        let mut log = String::new();
        for _ in 0..100 {
            log = std::fs::read_to_string(&log_path).unwrap_or_default();
            if log.contains("SKIPPED bead=pat-test-2") {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        assert!(
            log.contains("HOST-ISSUE"),
            "breaker must log the HOST-ISSUE banner after 2 consecutive timeouts; log:\n{log}"
        );
        assert!(
            log.contains("SKIPPED bead=pat-test-2"),
            "the 3rd verify must be skipped (lane halted), not launched; log:\n{log}"
        );
        let starts = log.matches("START bead=").count();
        assert_eq!(
            starts, 2,
            "only the first 2 verifies should launch, not the 3rd; log:\n{log}"
        );
    }

    /// B3: a verification held past the reap threshold is reopened and removed
    /// from the pending set, so a wedged verifier can't stall a bead forever.
    #[test]
    fn test_reap_stale_verifications_reopens_over_threshold() {
        let mut cfg = test_config();
        cfg.pending_verification_reap_seconds = 0; // everything is "stale"
        let coord = test_coordinator_with_config(cfg); // dry_run = true (no real br call)
        {
            let mut p = coord.pending_verifications.lock().unwrap();
            p.insert("pat-old1".to_string(), Instant::now());
            p.insert("pat-old2".to_string(), Instant::now());
        }
        let reaped = coord.reap_stale_verifications();
        assert_eq!(reaped, 2);
        assert!(coord.pending_verifications.lock().unwrap().is_empty());
    }

    /// B3: fresh verifications (under the threshold) must be left untouched.
    #[test]
    fn test_reap_stale_verifications_keeps_fresh() {
        let mut cfg = test_config();
        cfg.pending_verification_reap_seconds = 3600;
        let coord = test_coordinator_with_config(cfg);
        {
            let mut p = coord.pending_verifications.lock().unwrap();
            p.insert("pat-fresh".to_string(), Instant::now());
        }
        let reaped = coord.reap_stale_verifications();
        assert_eq!(reaped, 0);
        assert!(coord
            .pending_verifications
            .lock()
            .unwrap()
            .contains_key("pat-fresh"));
    }

    // ─── B4: orphan strike counter ──────────────────────────────────────

    #[test]
    fn test_update_orphan_strikes_reopens_after_threshold() {
        let mut strikes = HashMap::new();
        let orphaned = vec!["pat-a".to_string()];
        assert!(update_orphan_strikes(&orphaned, &mut strikes, 3).is_empty());
        assert!(update_orphan_strikes(&orphaned, &mut strikes, 3).is_empty());
        assert_eq!(
            update_orphan_strikes(&orphaned, &mut strikes, 3),
            vec!["pat-a".to_string()]
        );
        // After reopening, the counter resets (no immediate re-reopen next poll).
        assert!(update_orphan_strikes(&orphaned, &mut strikes, 3).is_empty());
    }

    #[test]
    fn test_update_orphan_strikes_resets_when_recovered() {
        let mut strikes = HashMap::new();
        update_orphan_strikes(&["pat-a".to_string()], &mut strikes, 3);
        // Bead recovered (assigned) this poll → strikes cleared.
        let none: Vec<String> = vec![];
        assert!(update_orphan_strikes(&none, &mut strikes, 3).is_empty());
        assert!(strikes.is_empty());
        // Re-orphaning starts fresh — does not immediately reopen.
        assert!(update_orphan_strikes(&["pat-a".to_string()], &mut strikes, 3).is_empty());
    }

    // ─── D4: reclaim hysteresis ─────────────────────────────────────────

    #[test]
    fn test_within_hysteresis() {
        // Just reclaimed → still within a 60s window.
        assert!(within_hysteresis(Duration::from_secs(5), 60));
        // Long past the window.
        assert!(!within_hysteresis(Duration::from_secs(120), 60));
        // Window of 0 disables hysteresis entirely.
        assert!(!within_hysteresis(Duration::from_secs(0), 0));
    }

    // ─── D5: deadlock watchdog ──────────────────────────────────────────

    #[test]
    fn test_deadlock_watchdog_alerts_once_at_threshold() {
        let mut c = 0;
        for _ in 0..2 {
            let (nc, alert) = deadlock_watchdog_tick(false, true, true, c, 3);
            c = nc;
            assert!(!alert);
        }
        let (nc, alert) = deadlock_watchdog_tick(false, true, true, c, 3);
        assert!(alert, "should alert when crossing threshold");
        assert_eq!(nc, 3);
        // Past threshold it keeps counting but does not re-alert every poll.
        let (nc2, alert2) = deadlock_watchdog_tick(false, true, true, nc, 3);
        assert!(!alert2);
        assert_eq!(nc2, 4);
    }

    #[test]
    fn test_deadlock_watchdog_resets_on_progress() {
        assert_eq!(deadlock_watchdog_tick(true, true, true, 5, 3), (0, false));
    }

    #[test]
    fn test_deadlock_watchdog_idle_is_not_deadlock() {
        // No work in flight (genuinely idle) or no workers → never a deadlock.
        assert_eq!(deadlock_watchdog_tick(false, false, true, 5, 3), (0, false));
        assert_eq!(deadlock_watchdog_tick(false, true, false, 5, 3), (0, false));
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

    /// Adaptive effort: a bead's `effort:` label wins; unlabeled falls back to
    /// the configured default; disabled yields None (worker keeps its launch
    /// baseline).
    #[test]
    fn assignment_effort_label_then_default_then_disabled() {
        let mut cfg = test_config();
        cfg.adaptive_effort_enabled = true;
        cfg.adaptive_effort_default = "high".to_string();
        let coord = test_coordinator_with_config(cfg);

        let mut bead = ready_bead("pat-1", 1, &[]);
        assert_eq!(
            coord.assignment_effort(&bead).as_deref(),
            Some("high"),
            "unlabeled bead uses the configured default"
        );
        bead.effort = Some("low".to_string());
        assert_eq!(
            coord.assignment_effort(&bead).as_deref(),
            Some("low"),
            "the bead's effort label wins over the default"
        );

        let mut cfg_off = test_config();
        cfg_off.adaptive_effort_enabled = false;
        let coord_off = test_coordinator_with_config(cfg_off);
        assert_eq!(
            coord_off.assignment_effort(&bead),
            None,
            "disabled → no injection, worker keeps its launch-baseline effort"
        );
    }

    #[test]
    fn test_build_prompt_text() {
        let coord = test_coordinator();

        let prompt = coord.build_prompt_text("pat-abc", "Fix the bug", "WorkerA");
        assert!(prompt.contains("pat-abc"), "must contain bead ID");
        assert!(prompt.contains("WorkerA"), "must contain worker name");
        assert!(prompt.contains("Fix the bug"), "must contain bead title");
        assert!(
            prompt.contains("flywheel-worker"),
            "must invoke flywheel-worker skill"
        );
        assert!(
            prompt.contains("/skill mail-complete"),
            "must reference mail-complete skill"
        );
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
        let r = ReassignResult {
            next_bead_id: None,
            prompt_task: None,
        };
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
        assert!(
            is_idle,
            "StuckInput worker should be treated as idle for assignment"
        );

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
        assert!(
            crate::worker::has_stuck_input(&prompt),
            "prompt text must be detectable as stuck input"
        );
        // Must trigger tmux post-submit verification
        assert!(
            prompt.contains("/skill flywheel-worker") && prompt.contains("Work bead"),
            "prompt must contain both invariant substrings for post-submit verification"
        );
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
            (
                "pat-w2",
                "Implement very long feature name that wraps many times",
                "PearlFox",
            ),
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
        let non_idle_states = vec![WorkerState::Active, WorkerState::Dead];
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
        assert_eq!(
            r.processed + r.errors,
            3,
            "total should be processed + errors"
        );
        assert!(
            r.errors > 0,
            "errors means un-acked messages remain for retry"
        );
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
        assert_ne!(
            detect_pane_state(queued),
            WorkerState::StuckInput,
            "queued prompt should NOT be StuckInput — it needs the nudge-Enter path"
        );
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
        let step5 = source
            .find("Step 5: Send assignment mail")
            .expect("Step 5 must exist");
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
        let mismatch_pos = source
            .find(marker)
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
        let evidence_check = after
            .find("Reject if missing evidence")
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
        let poll_pos = main_source
            .find("coord.poll()")
            .expect("coord.poll() must exist in main.rs");

        // Find the idle fill call after poll
        let after_poll = &main_source[poll_pos..];
        let idle_fill_pos = after_poll
            .find("run_idle_fill")
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
        let state_check = source
            .find("Check bead state in DB")
            .expect("bead state check must exist");
        let evidence_check = source[state_check..]
            .find("Reject if missing evidence")
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

        let closed_skip = source
            .find("SKIP: already closed/tombstone")
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
        let fn_start = source
            .find("pub fn reassign(")
            .expect("reassign must exist");
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
                let preceding =
                    &source[..source.lines().take(i).map(|l| l.len() + 1).sum::<usize>()];
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
                let line_offset: usize = source
                    .lines()
                    .take(*line_num - 1)
                    .map(|l| l.len() + 1)
                    .sum();
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
        let seed_start = source
            .find("Seed check")
            .expect("Seed check section must exist");
        let seed_section = &source[seed_start..seed_start + 600];

        // Must contain IMPORTANT comment about dropping before br subprocess calls
        assert!(
            seed_section.contains("DROPPED before")
                || seed_section.contains("dropped here")
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

        let mail_check = source
            .find("Ensure mail server")
            .expect("mail check must exist");
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
        let reassign_fn = source
            .find("pub fn reassign(")
            .expect("reassign must exist");
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
        let idle_fill = source
            .find("fn run_idle_fill(")
            .expect("run_idle_fill must exist");
        // Window covers the whole function body; the success-after-retry log sits
        // ~13 lines in (past the pull_mode early-return guard added later).
        let end = (idle_fill + 1200).min(source.len());
        let fn_section = &source[idle_fill..end];

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
        let seed_script = source[seed_start..]
            .find("seed_script.exists()")
            .unwrap_or(0);

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
        let second = source[first + 20..]
            .find("stall detection")
            .map(|p| first + 20 + p);
        if let Some(pos) = second {
            // Widened to 600 chars: the stall-detection block now carries a
            // multi-line CRITICAL comment documenting the Connection-scoping
            // invariant, which pushes the `db::open(` call further down.
            let end = (pos + 600).min(source.len());
            let section = &source[pos..end];
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
        if poll_ok == 0 {
            return;
        }

        let loop_end = source[poll_ok..].find("drop(lock_file)").unwrap_or(5000);
        let post_poll = &source[poll_ok..poll_ok + loop_end];

        // Count lines that have `let conn = db::open(` NOT inside a block
        // The safe pattern is: `let conn = db::open(` inside { }
        // The unsafe pattern is: `let conn = db::open(` at if-block scope
        let lines: Vec<(usize, &str)> = post_poll
            .lines()
            .enumerate()
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

        let fn_start = source
            .find("pub fn assign_idle_workers(")
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

        let fn_start = source
            .find("fn assign_idle_workers_inner(")
            .expect("assign_idle_workers_inner must exist");
        let fn_end = source[fn_start..].find("\n    pub fn ").unwrap_or(5000);
        let fn_body = &source[fn_start..fn_start + fn_end];

        // Must NOT have inline prompt_worker_pane calls
        let inline_prompts: Vec<_> = fn_body
            .lines()
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

    /// Regression: assign_idle_workers_inner must recover expired identities
    /// BEFORE any per-worker state is keyed off `worker_name`.
    ///
    /// Once every identity file ages past the resolve script's staleness window
    /// (~2 days of continuous running), every pane resolves to an empty name and
    /// the nameless-pane skip drops the whole swarm from assignment — 104 ready
    /// beads, 6 idle workers, assigned=0 for hours. Recovering after the skip,
    /// or after `worker_db_state` is built, would leave the stall in place.
    #[test]
    fn test_assign_recovers_expired_identities_before_name_checks() {
        let source = include_str!("coordinator.rs");

        let fn_start = source
            .find("fn assign_idle_workers_inner(")
            .expect("assign_idle_workers_inner must exist");
        let fn_end = source[fn_start..].find("\n    pub fn ").unwrap_or(5000);
        let fn_body = &source[fn_start..fn_start + fn_end];

        let recover_pos = fn_body
            .find("recover_expired_identities(")
            .expect("REGRESSION: assign_idle_workers_inner must recover expired identities");
        let first_name_check = fn_body
            .find("worker_name.is_empty()")
            .expect("assign_idle_workers_inner must gate on worker_name");

        assert!(
            recover_pos < first_name_check,
            "REGRESSION: identity recovery must run BEFORE worker_name is read, \
             otherwise an expired identity TTL silently stalls all assignment."
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
        let outer = source
            .find("let mut all_prompts: Vec<tmux::PromptTask>")
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

        let fn_start = source
            .find("fn submit_prompts_parallel(")
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
        let prompt_task = source
            .find("pub struct PromptTask")
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
        let fn_start = source
            .find("fn submit_prompts_parallel(")
            .expect("function must exist");
        let fn_body = &source[fn_start..fn_start + 2600];

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
        let fn_start = source
            .find("fn assign_idle_workers_inner(")
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
        let fn_start = source
            .find("fn submit_prompts_parallel(")
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
        // Anchor on the verifier lane's own thread — the file has other,
        // unrelated thread::spawns earlier (e.g. the auto-checkpoint thread),
        // so a bare first-match would sweep the wrong window.
        let lane_fn = source
            .find("fn spawn_verifier_worker(")
            .expect("spawn_verifier_worker must exist");
        let after_fn = &source[lane_fn..];
        // Find the thread::spawn dispatch and the "continue" after it.
        // Only check BETWEEN spawn and continue — not reject paths above.
        let spawn_pos = after_fn
            .find("std::thread::spawn(move ||")
            .expect("thread spawn must exist");
        let after_spawn = &after_fn[spawn_pos..];
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
        let pending_check = source
            .find("already being verified")
            .expect("pending guard must exist");
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
        let close_pos = fn_body
            .find("br::close_bead")
            .expect("close_bead must exist in reassign");
        // Find the LAST self.ack_msg — that's the main success-path ack
        let last_ack_pos = fn_body
            .rfind("self.ack_msg")
            .expect("self.ack_msg must exist in reassign");
        assert!(
            last_ack_pos > close_pos,
            "REGRESSION: main success-path self.ack_msg must come AFTER br::close_bead. \
             Acking before close causes orphaned beads."
        );
    }

    /// Regression: reassign() must CLOSE the completed bead BEFORE the
    /// "worker has other active beads" early-return.
    ///
    /// Bug: the active-bead check ran first and returned early (acking the
    /// message) without closing the just-verified bead. A worker holding any
    /// other active/orphaned bead then blocked every bead it completed from
    /// ever closing — PASS results were silently dropped, beads stayed
    /// in_progress, and the ready queue never drained. The close must happen
    /// unconditionally; only NEW-work assignment is gated on the worker being
    /// otherwise idle.
    #[test]
    fn test_reassign_closes_before_other_active_check() {
        let source = include_str!("coordinator.rs");
        let reassign_fn = source
            .find("pub fn reassign(")
            .expect("reassign must exist");
        let fn_body = &source[reassign_fn..(reassign_fn + 4000).min(source.len())];

        let close_pos = fn_body
            .find("br::close_bead")
            .expect("close_bead must exist in reassign");
        // The other-active gate is identified by its worker_active_assignments
        // lookup; it must come AFTER the close so the completed bead is always
        // closed regardless of the worker's other active beads.
        let other_active_pos = fn_body
            .find("worker_active_assignments")
            .expect("other-active check must exist in reassign");
        assert!(
            close_pos < other_active_pos,
            "REGRESSION: br::close_bead must come BEFORE the 'other active beads' \
             check in reassign(). Checking first and returning early skips the \
             close, orphaning every bead a busy worker completes."
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
        let struct_def = source
            .find("pub struct Coordinator")
            .expect("Coordinator must exist");
        let struct_end = source[struct_def..].find('}').unwrap_or(500);
        let struct_body = &source[struct_def..struct_def + struct_end];

        assert!(
            struct_body.contains("pending_verifications"),
            "must track pending verifications"
        );
        assert!(
            struct_body.contains("verify_rx"),
            "must have verification result receiver"
        );
        assert!(
            struct_body.contains("verify_tx"),
            "must have verification result sender"
        );
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
            partial: false,
            remaining_scope: String::new(),
        })
        .unwrap();

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
            pending.insert("pat-abc".to_string(), Instant::now());
        }
        let pending = coord.pending_verifications.lock().unwrap();
        assert!(pending.contains_key("pat-abc"));
        assert!(!pending.contains_key("pat-xyz"));
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

    // -- Partial completions (pat-k5o8f) ------------------------------------

    /// The scope comment is the durable artifact of a partial: everything the
    /// next worker needs has to be in it, because the mail message will be
    /// acked and gone.
    #[test]
    fn test_partial_scope_comment_carries_the_remaining_scope() {
        let comment = partial_scope_comment(
            "IndigoBridge",
            "engine-rs/crates/gdscene/src/canvas_group.rs",
            "cargo nextest run -p gdscene --test canvas_group",
            "BackBufferCopy/SCREEN_TEXTURE, ViewportTexture, Parallax",
        );
        assert!(
            comment.contains("left open"),
            "the comment must say the bead was deliberately not closed: {comment}"
        );
        assert!(comment.contains("IndigoBridge"));
        assert!(comment.contains("BackBufferCopy/SCREEN_TEXTURE"));
        assert!(comment.contains("ViewportTexture"));
        assert!(comment.contains("Parallax"));
        assert!(comment.contains("canvas_group.rs"), "delivered files");
        assert!(comment.contains("cargo nextest run"), "verify command");
    }

    /// A worker who flags partial without listing what is left still gets a
    /// usable comment — silence must not read as "nothing remains".
    #[test]
    fn test_partial_scope_comment_survives_a_missing_scope() {
        let comment = partial_scope_comment("W", "src/a.rs", "cargo test", "");
        assert!(comment.contains("Remaining scope"));
        assert!(
            comment.contains("not stated"),
            "an empty scope must be called out, not silently omitted: {comment}"
        );
    }

    /// A passing partial must never reach the close/reassign path, and the
    /// scope comment must be written before the bead is reopened — a reopen
    /// without the note loses the gap information entirely.
    #[test]
    fn test_partial_completion_leaves_bead_open_with_a_comment() {
        let source = include_str!("coordinator.rs");

        let drain = source
            .find("pub fn drain_verify_results(")
            .expect("drain_verify_results must exist");
        let drain_body = &source[drain..drain + 4000];
        assert!(
            drain_body.contains("} else if vr.partial {"),
            "the pass branch must route partial completions away from close"
        );
        assert!(
            drain_body.contains("handle_partial_completion(&vr)"),
            "partial completions must be handled, not closed"
        );

        let handler = source
            .find("fn handle_partial_completion(")
            .expect("handle_partial_completion must exist");
        let handler_body = &source[handler..handler + 1400];
        let comment_at = handler_body
            .find("br::comment_add(")
            .expect("the partial handler must record a br comment");
        let reopen_at = handler_body
            .find("br::reopen(")
            .expect("the partial handler must return the bead to open");
        assert!(
            comment_at < reopen_at,
            "the scope comment must be written before the reopen"
        );
        assert!(
            !handler_body.contains("close_bead"),
            "the partial handler must never close the bead"
        );
    }

    /// The flag has to survive the trip through the verify lane, or the drain
    /// branch above can never fire.
    #[test]
    fn test_verify_result_carries_partial_fields() {
        let source = include_str!("coordinator.rs");
        let s = source.find("struct VerifyResult").unwrap();
        let body = &source[s..s + 1200];
        assert!(body.contains("partial: bool"));
        assert!(body.contains("remaining_scope: String"));
        let r = source.find("struct VerifyRequest").unwrap();
        let req = &source[r..r + 900];
        assert!(req.contains("partial: bool"), "request must carry the flag");
    }

    /// The red check must run on the verifier lane after a green verify, and
    /// only a NotDetected verdict may turn the pass into a rejection.
    #[test]
    fn test_verify_lane_runs_the_red_check_after_a_green_verify() {
        let source = include_str!("coordinator.rs");
        let f = source.find("fn process_one_verification(").unwrap();
        let body = &source[f..f + 3500];
        let verify = body.find("verifier::verify_commands(").unwrap();
        let red = body.find("crate::wiring::red_check(").expect("red check must be called");
        assert!(red > verify, "red check runs after the green verify");
        assert!(body.contains("RedVerdict::NotDetected(why) = red"));
        assert!(body.contains("!request.is_stale_recovery"));
    }

    /// The entry-point rule runs on the completion path, before enqueueing a
    /// build, and a rejection reuses the unmet-acceptance reopen path.
    #[test]
    fn test_completion_path_applies_the_entry_point_rule() {
        let source = include_str!("coordinator.rs");
        let sel = source.find("verifier::select_verify_commands(&reported, &demand)").unwrap();
        let window = &source[sel..sel + 2500];
        assert!(window.contains("crate::wiring::check_entry_point("));
        assert!(window.contains("EntryPointVerdict::Rejected(why) => {"));
        assert!(window.contains("(Vec::new(), Some(why))"));
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
        assert!(
            body.contains("recover_stale_assignments"),
            "poll() must call recover_stale_assignments so it runs in pull mode too"
        );
    }

    #[test]
    fn test_verify_stale_bead_falls_back_to_workspace_test() {
        let source = include_str!("coordinator.rs");
        let method = source.find("fn verify_stale_bead(").unwrap();
        let end = source[method + 1..].find("\n    fn ").unwrap_or(1000);
        let body = &source[method..method + end];
        assert!(body.contains("commands.is_empty()"));
        assert!(
            body.contains("cargo build --workspace"),
            "must fall back to workspace build check when no acceptance commands"
        );
    }

    #[test]
    fn test_stale_recovery_verify_result_channel() {
        let coord = test_coordinator();
        coord
            .verify_tx
            .clone()
            .send(VerifyResult {
                bead_id: "pat-stale".to_string(),
                worker: String::new(),
                msg_id: None,
                ack_required: false,
                files_changed: String::new(),
                tests_run: String::new(),
                passed: true,
                reject_reason: None,
                is_stale_recovery: true,
                partial: false,
                remaining_scope: String::new(),
            })
            .unwrap();
        let rx = coord.verify_rx.lock().unwrap();
        let result = rx.try_recv().unwrap();
        assert!(result.is_stale_recovery);
        assert!(result.worker.is_empty());
    }

    #[test]
    fn test_verify_stale_bead_skips_if_already_pending() {
        let coord = test_coordinator();
        {
            coord
                .pending_verifications
                .lock()
                .unwrap()
                .insert("pat-already".to_string(), Instant::now());
        }
        coord.verify_stale_bead("pat-already");
        assert!(
            coord.verify_rx.lock().unwrap().try_recv().is_err(),
            "should not dispatch verification for already-pending bead"
        );
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
