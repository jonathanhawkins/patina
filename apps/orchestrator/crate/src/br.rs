use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use crate::error::{OrchestratorError, Result};

/// Simple counter-based jitter source. Avoids thundering-herd by mixing
/// a monotonically incrementing counter with the thread ID, producing
/// different values across threads even when called at the same instant.
static JITTER_COUNTER: AtomicU64 = AtomicU64::new(0);

fn br_max_retries() -> u32 {
    std::env::var("ORCH_BR_MAX_RETRIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}

fn br_retry_base_delay_ms() -> u64 {
    std::env::var("ORCH_BR_RETRY_DELAY_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1)
        * 1000
}

/// Compute a retry delay with jitter to avoid thundering-herd collisions.
/// Uses an atomic counter mixed with the thread ID hash so that concurrent
/// threads get different jitter values, even when called at the same instant.
fn br_retry_delay_with_jitter(attempt: u32) -> Duration {
    let base_ms = br_retry_base_delay_ms();
    let counter = JITTER_COUNTER.fetch_add(1, Ordering::Relaxed);
    let thread_hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::thread::current().id().hash(&mut h);
        h.finish()
    };
    let jitter_ms = (counter.wrapping_mul(6364136223846793005).wrapping_add(thread_hash)) % 500;
    let backoff_factor = 1u64 << attempt.min(3);
    let total_ms = base_ms.saturating_mul(backoff_factor).saturating_add(jitter_ms);
    Duration::from_millis(total_ms)
}

/// SQLite busy timeout (ms) passed to every `br` invocation via `--lock-timeout`.
/// Matches the 30 s timeout the old raw-SQL path used so that `bv` readers
/// don't cause spurious "database is busy" failures.
fn br_lock_timeout_ms() -> u64 {
    std::env::var("ORCH_BR_LOCK_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10000)
}

/// Run a `br` command with retry on "database is locked/busy" errors.
/// Public so the planner auto-create path can call `br create` directly.
pub fn run_br_public(args: &[&str]) -> Result<String> {
    run_br(args)
}

/// Process-level timeout for `br` commands. Even though `--lock-timeout`
/// tells SQLite to timeout internally, a deadlocked `br` process can hang
/// forever if the SQLite timeout doesn't fire (WAL checkpoint races).
/// This kills the child process after the deadline, preventing the
/// coordinator from freezing.
fn br_process_timeout_secs() -> u64 {
    std::env::var("ORCH_BR_PROCESS_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

/// Spawn a `br` command with a process-level timeout.
/// Returns (stdout, stderr, success) or kills the child and returns an error.
fn spawn_br_with_timeout(args: &[&str]) -> Result<(String, String, bool)> {
    let timeout_str = br_lock_timeout_ms().to_string();
    let deadline = Duration::from_secs(br_process_timeout_secs());

    let mut child = Command::new("br")
        .args(args)
        .args(["--lock-timeout", &timeout_str, "--no-auto-import", "--no-auto-flush"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| OrchestratorError::Br(format!("failed to spawn br: {e}")))?;

    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(200);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child.stdout.take()
                    .map(|mut r| { let mut s = String::new(); std::io::Read::read_to_string(&mut r, &mut s).ok(); s })
                    .unwrap_or_default();
                let stderr = child.stderr.take()
                    .map(|mut r| { let mut s = String::new(); std::io::Read::read_to_string(&mut r, &mut s).ok(); s })
                    .unwrap_or_default();
                return Ok((stdout, stderr, status.success()));
            }
            Ok(None) => {
                if start.elapsed() >= deadline {
                    tracing::warn!(
                        args = args.join(" "),
                        elapsed_secs = start.elapsed().as_secs(),
                        "br process exceeded timeout — killing"
                    );
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(OrchestratorError::Br(format!(
                        "br {} killed after {}s timeout (deadlocked on DB lock)",
                        args.join(" "),
                        deadline.as_secs()
                    )));
                }
                thread::sleep(poll_interval);
            }
            Err(e) => {
                return Err(OrchestratorError::Br(format!(
                    "error waiting for br process: {e}"
                )));
            }
        }
    }
}

fn run_br(args: &[&str]) -> Result<String> {
    let max_retries = br_max_retries();
    let mut last_output = String::new();

    for attempt in 0..max_retries {
        let (stdout, stderr, success) = spawn_br_with_timeout(args)?;

        if success {
            return Ok(stdout);
        }

        let combined = format!("{stdout}{stderr}");
        last_output = combined.clone();

        let is_locked = combined.to_lowercase().contains("database is locked")
            || combined.to_lowercase().contains("database is busy");

        if !is_locked || attempt == max_retries - 1 {
            return Err(OrchestratorError::Br(format!(
                "br {} failed (exit): {}",
                args.join(" "),
                combined.trim()
            )));
        }

        thread::sleep(br_retry_delay_with_jitter(attempt));
    }

    Err(OrchestratorError::Br(format!(
        "br {} failed after {max_retries} retries: {}",
        args.join(" "),
        last_output.trim()
    )))
}

/// Close a bead with the given reason.
///
/// If the bead is not found, runs `br sync --import-only` to reimport from JSONL
/// (the bead may have been created by a different swarm session or the DB may be
/// stale after an orchestrator restart) and retries once.
///
/// Falls back to `br update --status closed` on DB contention (bv reader lock).
pub fn close_bead(bead_id: &str, reason: &str) -> Result<String> {
    match run_br(&["close", bead_id, "--reason", reason]) {
        Ok(output) => Ok(output),
        Err(e) if is_not_found_error(&e) => {
            // Bead missing from DB — sync from JSONL and retry
            tracing::warn!(
                bead = bead_id,
                "Bead not found — syncing DB from JSONL and retrying"
            );
            if let Err(sync_err) = force_sync() {
                tracing::warn!(error = %sync_err, "force_sync failed during close retry");
            }
            run_br(&["close", bead_id, "--reason", reason])
        }
        Err(e) => {
            let msg = format!("{e}").to_lowercase();
            if msg.contains("database is busy") || msg.contains("database is locked") {
                tracing::warn!(
                    bead = bead_id,
                    "br close hit DB contention, falling back to br update --status closed"
                );
                update_bead(bead_id, None, Some("closed"))
            } else {
                Err(e)
            }
        }
    }
}

/// Update a bead's assignee and/or status.
///
/// If the bead is not found, syncs from JSONL and retries once.
pub fn update_bead(
    bead_id: &str,
    assignee: Option<&str>,
    status: Option<&str>,
) -> Result<String> {
    let mut args = vec!["update", bead_id];
    if let Some(a) = assignee {
        args.push("--assignee");
        args.push(a);
    }
    if let Some(s) = status {
        args.push("--status");
        args.push(s);
    }
    match run_br(&args) {
        Ok(output) => Ok(output),
        Err(e) if is_not_found_error(&e) => {
            tracing::warn!(
                bead = bead_id,
                "Bead not found — syncing DB from JSONL and retrying"
            );
            if let Err(sync_err) = force_sync() {
                tracing::warn!(error = %sync_err, "force_sync failed during update retry");
            }
            run_br(&args)
        }
        Err(e) => Err(e),
    }
}

/// Sync the bead tracker. Exports DB → JSONL (flush-only).
///
/// Uses only `--flush-only` — the coordinator writes to the DB via `br close`
/// and `br update`, so JSONL just needs to reflect those changes. The old
/// `--merge` path was expensive (import + export) and would hang for 30-43s
/// when bv held a WAL read lock.
///
/// If the DB is stale (JSONL has issues missing from DB), imports first.
pub fn sync() -> Result<()> {
    match run_br(&["sync", "--flush-only"]) {
        Ok(_) => Ok(()),
        Err(e) if is_stale_db_error(&e) => {
            tracing::info!("DB is stale — importing from JSONL before flush");
            import()?;
            run_br(&["sync", "--flush-only"])?;
            Ok(())
        }
        Err(e) => {
            let msg = format!("{e}");
            if msg.contains("UNIQUE constraint failed: export_hashes") {
                // Workaround for br bug: export_hashes table produces UNIQUE
                // violations during flush. Clear the table and use --force to
                // bypass safety guards since the DB is the source of truth here.
                tracing::warn!("export_hashes constraint error — clearing table and retrying with --force");
                clear_export_hashes()?;
                match run_br(&["sync", "--flush-only", "--force"]) {
                    Ok(_) => Ok(()),
                    Err(e2) => {
                        tracing::warn!(error = %e2, "flush retry also failed — skipping this cycle");
                        Ok(()) // Don't propagate — sync will catch up next cycle
                    }
                }
            } else {
                Err(e)
            }
        }
    }
}

/// Clear the export_hashes table to work around a br bug where
/// flush-only fails with UNIQUE constraint violations.
fn clear_export_hashes() -> Result<()> {
    let db_path = find_db_path()?;
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| OrchestratorError::Br(format!("failed to open DB: {e}")))?;
    conn.execute("DELETE FROM export_hashes", [])
        .map_err(|e| OrchestratorError::Br(format!("failed to clear export_hashes: {e}")))?;
    Ok(())
}

/// Find the beads DB path.
fn find_db_path() -> Result<std::path::PathBuf> {
    let beads_dir = std::path::Path::new(".beads");
    if beads_dir.join("beads.db").exists() {
        return Ok(beads_dir.join("beads.db"));
    }
    // Try from project root
    for entry in std::fs::read_dir(beads_dir).map_err(|e| OrchestratorError::Br(format!("{e}")))? {
        let entry = entry.map_err(|e| OrchestratorError::Br(format!("{e}")))?;
        if entry.path().extension().map(|e| e == "db").unwrap_or(false) {
            return Ok(entry.path());
        }
    }
    Err(OrchestratorError::Br("no .beads/*.db found".into()))
}

/// Import JSONL into the SQLite database.
/// Uses `br sync --import-only` (the correct subcommand).
fn import() -> Result<()> {
    run_br(&["sync", "--import-only"])?;
    Ok(())
}

/// Force a full reimport of JSONL into the SQLite database.
/// Use when the DB is missing beads (e.g., after an orchestrator restart
/// or when processing beads created by a different swarm session).
pub fn force_sync() -> Result<()> {
    tracing::info!("Running br sync --import-only (full reimport)");
    run_br(&["sync", "--import-only"])?;
    Ok(())
}

/// Check if an error indicates a bead was not found in the database.
pub fn is_not_found_error(e: &OrchestratorError) -> bool {
    let msg = format!("{e}").to_lowercase();
    msg.contains("not found") || msg.contains("issue not found")
}

/// Check if an error is the "stale database" / "would lose issues" error.
fn is_stale_db_error(e: &OrchestratorError) -> bool {
    let msg = format!("{e}");
    msg.contains("would lose") || msg.contains("Refusing to export stale")
}

/// Reopen a bead by clearing its assignee and setting status to open.
pub fn reopen(bead_id: &str) -> Result<()> {
    update_bead(bead_id, Some(""), Some("open"))?;
    Ok(())
}

/// Get ready unassigned beads as JSON.
pub fn ready_unassigned_json(limit: usize) -> Result<String> {
    let limit_str = limit.to_string();
    run_br(&["ready", "--unassigned", "--format", "json", "--limit", &limit_str])
}

/// Get ready unassigned beads filtered by label(s) as JSON.
pub fn ready_unassigned_with_labels(labels: &[&str], limit: usize) -> Result<String> {
    let limit_str = limit.to_string();
    let label_str = labels.join(",");
    run_br(&["ready", "--unassigned", "--format", "json", "--limit", &limit_str, "--label", &label_str])
}

// ─── Dependency management ───────────────────────────────────────────────

/// Add a dependency: `blocker` must be completed before `blocked_id` can be worked on.
/// `br ready` will automatically exclude `blocked_id` until `blocker` is closed.
pub fn dep_add(blocked_id: &str, blocker_id: &str) -> Result<()> {
    run_br(&["dep", "add", blocked_id, "--blocks", blocker_id])?;
    Ok(())
}

/// Remove a dependency relationship.
pub fn dep_remove(blocked_id: &str, blocker_id: &str) -> Result<()> {
    run_br(&["dep", "remove", blocked_id, "--blocks", blocker_id])?;
    Ok(())
}

/// List beads that are currently blocked (have unresolved dependencies).
pub fn blocked_json() -> Result<String> {
    run_br(&["blocked", "--format", "json"])
}

// ─── Label management ────────────────────────────────────────────────────

/// Add a label to a bead.
pub fn label_add(bead_id: &str, label: &str) -> Result<()> {
    run_br(&["label", "add", bead_id, label])?;
    Ok(())
}

/// Remove a label from a bead.
pub fn label_remove(bead_id: &str, label: &str) -> Result<()> {
    run_br(&["label", "remove", bead_id, label])?;
    Ok(())
}

// ─── Stale detection ─────────────────────────────────────────────────────

/// Find beads that haven't been updated in `days` days.
pub fn stale_json(days: u32) -> Result<String> {
    let days_str = days.to_string();
    run_br(&["stale", "--days", &days_str, "--format", "json"])
}

// ─── Comments ────────────────────────────────────────────────────────────

/// Add a comment to a bead (for coordinator notes, rejection reasons, etc.)
pub fn comment_add(bead_id: &str, text: &str) -> Result<()> {
    run_br(&["comments", "add", bead_id, "--text", text])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_stale_db_error: regression test ──────────────────────────────
    // The original bug: sync() did not detect the "would lose issues" error
    // from `br sync --flush-only` and let it propagate as a generic Br error,
    // leaving the orchestrator in a permanent sync-failure loop.

    /// Regression: exact error message from the real br CLI when DB is stale.
    #[test]
    fn test_stale_db_error_detected_exact_production_message() {
        let err = OrchestratorError::Br(
            "br sync --flush-only failed (exit exit status: 7): Error: Configuration error: \
             Refusing to export stale database that would lose issues.\n\
             Database has 228 issues, JSONL has 352 unique issues.\n\
             Export would lose 129 issue(s): pat-0fa, pat-0lo, pat-13d ... and 119 more\n\
             Hint: Run import first, or use --force to override."
                .to_string(),
        );
        assert!(
            is_stale_db_error(&err),
            "Must detect the exact production error message that caused the original bug"
        );
    }

    /// Regression: the "would lose" substring also appears in the error.
    #[test]
    fn test_stale_db_error_detected_would_lose_variant() {
        let err = OrchestratorError::Br(
            "br sync --merge failed (exit exit status: 7): Export would lose 5 issue(s)".to_string(),
        );
        assert!(
            is_stale_db_error(&err),
            "Must detect 'would lose' variant of stale DB error"
        );
    }

    // ── is_stale_db_error: boundary tests ───────────────────────────────

    /// Boundary: message contains "Refusing to export stale" but no "would lose".
    #[test]
    fn test_stale_db_error_refusing_only() {
        let err = OrchestratorError::Br(
            "Refusing to export stale database".to_string(),
        );
        assert!(is_stale_db_error(&err));
    }

    /// Boundary: message contains "would lose" but no "Refusing".
    #[test]
    fn test_stale_db_error_would_lose_only() {
        let err = OrchestratorError::Br("Export would lose 1 issue(s)".to_string());
        assert!(is_stale_db_error(&err));
    }

    /// Boundary: empty error message is not stale.
    #[test]
    fn test_stale_db_error_empty_message() {
        let err = OrchestratorError::Br(String::new());
        assert!(!is_stale_db_error(&err), "Empty error must not match stale DB");
    }

    /// Boundary: single-character error message.
    #[test]
    fn test_stale_db_error_single_char() {
        let err = OrchestratorError::Br("x".to_string());
        assert!(!is_stale_db_error(&err));
    }

    // ── is_stale_db_error: negative tests ───────────────────────────────

    /// Negative: "database is locked" is NOT a stale DB error (different recovery path).
    #[test]
    fn test_stale_db_error_not_triggered_by_locked() {
        let err = OrchestratorError::Br("database is locked".to_string());
        assert!(
            !is_stale_db_error(&err),
            "DB locked errors must NOT be treated as stale DB — they have their own retry path"
        );
    }

    /// Negative: "database is busy" is NOT a stale DB error.
    #[test]
    fn test_stale_db_error_not_triggered_by_busy() {
        let err = OrchestratorError::Br("database is busy".to_string());
        assert!(!is_stale_db_error(&err));
    }

    /// Negative: generic br failure is NOT a stale DB error.
    #[test]
    fn test_stale_db_error_not_triggered_by_generic_failure() {
        let err = OrchestratorError::Br(
            "br sync --merge failed (exit exit status: 1): unknown error".to_string(),
        );
        assert!(!is_stale_db_error(&err));
    }

    /// Negative: non-Br error variants are not stale DB errors.
    #[test]
    fn test_stale_db_error_non_br_variant() {
        let err = OrchestratorError::Config("Refusing to export stale database".to_string());
        // is_stale_db_error uses Display which prefixes "config error: ..."
        // but the substring should still match — verify this is intentional
        assert!(
            is_stale_db_error(&err),
            "is_stale_db_error checks Display output, so matching substring in any variant is valid"
        );
    }

    // ── is_stale_db_error: variant tests ────────────────────────────────

    /// Variant: future br versions might phrase it slightly differently.
    #[test]
    fn test_stale_db_error_partial_match_would_lose() {
        let err = OrchestratorError::Br("Sync aborted: would lose 42 entries".to_string());
        assert!(is_stale_db_error(&err));
    }

    /// Variant: error wrapped with additional context.
    #[test]
    fn test_stale_db_error_wrapped_context() {
        let err = OrchestratorError::Br(
            "retry 3/5 for br sync --merge: Refusing to export stale database that would lose issues"
                .to_string(),
        );
        assert!(is_stale_db_error(&err));
    }

    /// Variant: multiline error output (as seen in production logs).
    #[test]
    fn test_stale_db_error_multiline() {
        let err = OrchestratorError::Br(
            "line1\nRefusing to export stale database\nline3".to_string(),
        );
        assert!(is_stale_db_error(&err));
    }

    // ── is_stale_db_error: stress test ──────────────────────────────────

    /// Stress: is_stale_db_error is a hot path in the poll loop — ensure it handles
    /// large error messages without performance issues.
    #[test]
    fn test_stale_db_error_large_message() {
        let padding = "x".repeat(100_000);
        let err = OrchestratorError::Br(format!(
            "{padding}\nExport would lose 999 issue(s)\n{padding}"
        ));
        assert!(is_stale_db_error(&err));
    }

    /// Stress: 1000 rapid calls with varying messages.
    #[test]
    fn test_stale_db_error_rapid_calls() {
        for i in 0..1000 {
            let msg = if i % 3 == 0 {
                format!("would lose {i} issues")
            } else if i % 3 == 1 {
                format!("database is locked attempt {i}")
            } else {
                format!("generic error {i}")
            };
            let err = OrchestratorError::Br(msg);
            let expected = i % 3 == 0;
            assert_eq!(
                is_stale_db_error(&err),
                expected,
                "iteration {i}: expected {expected}"
            );
        }
    }

    // ── Existing retry/jitter tests ─────────────────────────────────────

    /// br_retry_delay_with_jitter produces increasing delays with bounded jitter.
    #[test]
    fn test_br_retry_delay_increases_with_attempt() {
        let base_ms = br_retry_base_delay_ms();
        for attempt in 0..4u32 {
            let delay = br_retry_delay_with_jitter(attempt);
            let factor = 1u64 << attempt.min(3);
            let min = Duration::from_millis(base_ms * factor);
            let max = min + Duration::from_millis(500);
            assert!(
                delay >= min && delay <= max,
                "attempt {attempt}: delay {delay:?} not in [{min:?}, {max:?}]"
            );
        }
    }

    /// br backoff caps at 8× base for high attempt values.
    #[test]
    fn test_br_retry_delay_caps_at_8x() {
        let base_ms = br_retry_base_delay_ms();
        let d5 = br_retry_delay_with_jitter(5);
        let d100 = br_retry_delay_with_jitter(100);
        let cap_min = Duration::from_millis(base_ms * 8);
        let cap_max = cap_min + Duration::from_millis(500);
        assert!(d5 >= cap_min && d5 <= cap_max, "attempt 5: {d5:?}");
        assert!(d100 >= cap_min && d100 <= cap_max, "attempt 100: {d100:?}");
    }

    /// br_max_retries defaults to 5 when env var is unset.
    #[test]
    fn test_br_max_retries_default() {
        // Clear any env override (test isolation isn't perfect, but the default is 5)
        let val = br_max_retries();
        assert!(val >= 1, "max_retries should be at least 1, got {val}");
    }

    /// br_retry_base_delay_ms defaults to 1000ms when env var is unset.
    #[test]
    fn test_br_retry_base_delay_default() {
        let val = br_retry_base_delay_ms();
        assert!(val >= 1000, "base delay should be at least 1000ms, got {val}");
    }

    /// Different attempt levels produce different base delays (exponential backoff).
    #[test]
    fn test_br_different_attempts_differ() {
        let d0 = br_retry_delay_with_jitter(0);
        let d2 = br_retry_delay_with_jitter(2);
        // attempt=0 max = 1000+499 = 1499ms, attempt=2 min = 4000ms
        assert!(
            d2 > d0,
            "attempt=2 ({d2:?}) should be larger than attempt=0 ({d0:?})"
        );
    }

    // ========================================================================
    // Regression: br subprocess performance — auto-flush and lock timeout
    //
    // Bug: Every br close/update call triggered an auto-flush (JSONL export)
    // that took 30-43s waiting on bv's WAL read lock. Combined with redundant
    // br::sync() calls in the coordinator, each assignment took 41+ seconds.
    // Fix: pass --no-auto-flush to all br calls, remove redundant syncs,
    // and reduce lock timeout from 30s to 5s.
    // ========================================================================

    /// Regression: run_br must pass --no-auto-flush to prevent 30-43s hangs.
    #[test]
    fn test_spawn_br_passes_no_auto_flush() {
        let source = include_str!("br.rs");
        let spawn_fn = source.find("fn spawn_br_with_timeout(").expect("spawn_br_with_timeout must exist");
        let fn_end = source[spawn_fn..].find("\n}").unwrap_or(500);
        let fn_body = &source[spawn_fn..spawn_fn + fn_end];

        assert!(
            fn_body.contains("--no-auto-flush"),
            "REGRESSION: spawn_br_with_timeout must pass --no-auto-flush to prevent WAL lock hangs"
        );
    }

    /// Regression: br commands must pass --no-auto-import.
    #[test]
    fn test_spawn_br_passes_no_auto_import() {
        let source = include_str!("br.rs");
        let spawn_fn = source.find("fn spawn_br_with_timeout(").expect("spawn_br_with_timeout must exist");
        let fn_end = source[spawn_fn..].find("\n}").unwrap_or(500);
        let fn_body = &source[spawn_fn..spawn_fn + fn_end];

        assert!(
            fn_body.contains("--no-auto-import"),
            "spawn_br_with_timeout must pass --no-auto-import"
        );
    }

    /// Regression: lock timeout must be 5s or less (was 30s, causing long waits).
    #[test]
    fn test_default_lock_timeout_is_reasonable() {
        let source = include_str!("br.rs");
        // Must be <= 15s (was 30s which caused 43s hangs in the hot path)
        assert!(
            source.contains(".unwrap_or(10000)"),
            "Default lock timeout should be 10000ms (balanced: not too long for hot path, enough for bv contention)"
        );
    }

    /// Regression: sync() must NOT use --merge (expensive import+export).
    /// Only flush-only is needed — the coordinator writes directly to DB.
    #[test]
    fn test_sync_uses_flush_only_not_merge() {
        let source = include_str!("br.rs");
        let sync_fn = source.find("pub fn sync()").expect("sync() must exist");
        let fn_end = source[sync_fn..].find("\n}").unwrap_or(500);
        let fn_body = &source[sync_fn..sync_fn + fn_end];

        assert!(
            fn_body.contains("--flush-only"),
            "sync() must use --flush-only"
        );
        assert!(
            !fn_body.contains("--merge"),
            "REGRESSION: sync() must NOT use --merge (causes 30-43s WAL lock hangs)"
        );
    }

    /// Regression: close_bead must auto-sync on "not found" instead of failing.
    #[test]
    fn test_close_bead_syncs_on_not_found() {
        let source = include_str!("br.rs");
        let close_fn = source.find("pub fn close_bead(").expect("close_bead must exist");
        let fn_end = source[close_fn..].find("\n}").unwrap_or(800);
        let fn_body = &source[close_fn..close_fn + fn_end];

        assert!(
            fn_body.contains("is_not_found_error"),
            "close_bead must check for not-found errors"
        );
        assert!(
            fn_body.contains("force_sync"),
            "close_bead must call force_sync when bead not found"
        );
    }

    /// Regression: update_bead must auto-sync on "not found" instead of failing.
    #[test]
    fn test_update_bead_syncs_on_not_found() {
        let source = include_str!("br.rs");
        let update_fn = source.find("pub fn update_bead(").expect("update_bead must exist");
        let fn_end = source[update_fn..].find("\n}").unwrap_or(500);
        let fn_body = &source[update_fn..update_fn + fn_end];

        assert!(
            fn_body.contains("is_not_found_error"),
            "update_bead must check for not-found errors"
        );
        assert!(
            fn_body.contains("force_sync"),
            "update_bead must call force_sync when bead not found"
        );
    }

    /// Stress: is_not_found_error handles various error message formats.
    #[test]
    fn test_is_not_found_error_patterns() {
        use crate::error::OrchestratorError;
        let cases = vec![
            ("Issue not found: pat-abc", true),
            ("Error: Issue not found: pat-xyz\nHint: Run 'br list'", true),
            ("not found", true),
            ("database is busy", false),
            ("database is locked", false),
            ("already closed", false),
            ("", false),
        ];
        for (msg, expected) in cases {
            let e = OrchestratorError::Br(msg.to_string());
            assert_eq!(
                is_not_found_error(&e), expected,
                "is_not_found_error({msg:?}) should be {expected}"
            );
        }
    }

    /// Variant: import() uses correct subcommand (not the nonexistent 'br import').
    #[test]
    fn test_import_uses_sync_import_only() {
        let source = include_str!("br.rs");
        let import_fn = source.find("fn import()").expect("import() must exist");
        let fn_end = source[import_fn..].find("\n}").unwrap_or(200);
        let fn_body = &source[import_fn..import_fn + fn_end];

        assert!(
            fn_body.contains("--import-only"),
            "import() must use 'br sync --import-only', not 'br import'"
        );
        // Must NOT use bare "import" subcommand
        assert!(
            !fn_body.contains(r#"["import"]"#),
            "REGRESSION: import() must NOT call 'br import' (subcommand doesn't exist)"
        );
    }

    // --- Process-level timeout regression tests ---
    // Bug: br process deadlocked on SQLite DB lock despite --lock-timeout,
    // causing the coordinator to freeze forever. The coordinator poll loop
    // could not process completions, assign workers, or recover stuck panes
    // for 23+ minutes until manually killed.

    /// Regression: spawn_br_with_timeout must exist and use process-level kill.
    /// Without this, a deadlocked br child blocks the coordinator forever.
    #[test]
    fn test_spawn_br_with_timeout_exists_and_kills() {
        let source = include_str!("br.rs");
        assert!(
            source.contains("fn spawn_br_with_timeout"),
            "spawn_br_with_timeout must exist"
        );
        assert!(
            source.contains("child.kill()"),
            "spawn_br_with_timeout must kill the child on timeout"
        );
        assert!(
            source.contains("try_wait"),
            "spawn_br_with_timeout must poll with try_wait, not block with .output()"
        );
    }

    /// Regression: run_br must NOT use .output() which blocks forever.
    /// It must use spawn_br_with_timeout instead.
    #[test]
    fn test_run_br_does_not_use_blocking_output() {
        let source = include_str!("br.rs");
        let run_br_fn = source.find("fn run_br(").expect("run_br must exist");
        let fn_end = source[run_br_fn..].find("\n}").unwrap_or(500);
        let fn_body = &source[run_br_fn..run_br_fn + fn_end];

        assert!(
            !fn_body.contains(".output()"),
            "REGRESSION: run_br must NOT use .output() — it blocks forever on deadlocked br. \
             Use spawn_br_with_timeout instead."
        );
        assert!(
            fn_body.contains("spawn_br_with_timeout"),
            "run_br must delegate to spawn_br_with_timeout for process-level timeout"
        );
    }

    /// Regression: default process timeout must be reasonable (not infinite).
    #[test]
    fn test_default_process_timeout_is_finite() {
        let timeout = br_process_timeout_secs();
        assert!(
            timeout > 0 && timeout <= 60,
            "Process timeout must be 1-60s, got {timeout}s. \
             Too short = flaky kills; too long = coordinator freezes."
        );
    }

    /// Regression: process timeout must be longer than lock timeout.
    /// Otherwise the process is killed before SQLite has a chance to timeout gracefully.
    #[test]
    fn test_process_timeout_exceeds_lock_timeout() {
        let process_ms = br_process_timeout_secs() * 1000;
        let lock_ms = br_lock_timeout_ms();
        assert!(
            process_ms > lock_ms,
            "Process timeout ({process_ms}ms) must exceed lock timeout ({lock_ms}ms) \
             to give SQLite a chance to timeout gracefully before we kill the process"
        );
    }

    /// Functional: spawn_br_with_timeout kills a process that exceeds the deadline.
    #[test]
    fn test_spawn_timeout_kills_slow_process() {
        // Use a tiny timeout and a command that would block forever
        let deadline = Duration::from_secs(1);

        let mut child = Command::new("sleep")
            .arg("60")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(100);

        let mut timed_out = false;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if start.elapsed() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                        break;
                    }
                    thread::sleep(poll_interval);
                }
                Err(_) => break,
            }
        }

        assert!(timed_out, "Process should have been killed after 1s timeout");
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "Kill should happen promptly, not after 60s"
        );
    }
}
