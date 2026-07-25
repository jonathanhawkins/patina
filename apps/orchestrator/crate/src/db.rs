use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::error::{OrchestratorError, Result};

/// Counter-based jitter source (same approach as br.rs).
static DB_JITTER_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Bead status as tracked in the issues table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeadStatus {
    Open,
    InProgress,
    Closed,
    Tombstone,
}

impl BeadStatus {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "in_progress" => Some(Self::InProgress),
            "closed" => Some(Self::Closed),
            "tombstone" => Some(Self::Tombstone),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Closed => "closed",
            Self::Tombstone => "tombstone",
        }
    }
}

/// Core bead information from the issues table.
#[derive(Debug, Clone)]
pub struct BeadInfo {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: BeadStatus,
    pub assignee: Option<String>,
    pub updated_at: Option<String>,
    pub priority: i32,
    /// `crate:*` labels on the bead, for collision-aware assignment.
    pub crate_labels: Vec<String>,
    /// Reasoning-effort level from an `effort:<level>` label (level only, e.g.
    /// "high"), used for adaptive per-bead worker effort. None when unlabeled.
    pub effort: Option<String>,
    /// Number of open beads a close of this bead would directly unblock.
    pub unblock_impact: i64,
}

fn db_max_retries() -> u32 {
    std::env::var("ORCH_DB_MAX_RETRIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}

fn db_retry_base_delay_ms() -> u64 {
    std::env::var("ORCH_DB_RETRY_DELAY_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1)
        * 1000
}

/// Compute a retry delay with jitter to avoid thundering-herd collisions.
/// Uses an atomic counter mixed with thread ID hash for proper cross-thread dispersion.
fn retry_delay_with_jitter(attempt: u32) -> Duration {
    let base_ms = db_retry_base_delay_ms();
    let counter = DB_JITTER_COUNTER.fetch_add(1, Ordering::Relaxed);
    let thread_hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::thread::current().id().hash(&mut h);
        h.finish()
    };
    let jitter_ms = (counter
        .wrapping_mul(6364136223846793005)
        .wrapping_add(thread_hash))
        % 500;
    // Exponential backoff capped at 8× base, plus jitter
    let backoff_factor = 1u64 << attempt.min(3); // 1, 2, 4, 8
    let total_ms = base_ms
        .saturating_mul(backoff_factor)
        .saturating_add(jitter_ms);
    Duration::from_millis(total_ms)
}

/// Find the beads database file in the .beads directory.
fn find_db_path(project_root: &Path) -> Result<std::path::PathBuf> {
    let beads_dir = project_root.join(".beads");
    let db_path = beads_dir.join("beads.db");
    if db_path.exists() {
        return Ok(db_path);
    }

    Err(OrchestratorError::DbNotFound(beads_dir))
}

/// Open a connection to the beads database with WAL mode and timeout.
/// Uses read-write mode to avoid WAL checkpoint blocking on macOS with
/// SQLITE_OPEN_READ_ONLY (which holds shared locks on the SHM file).
/// All queries issued through this connection are SELECTs only.
pub fn open(project_root: &Path) -> Result<Connection> {
    let db_path = find_db_path(project_root)?;
    let conn = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.busy_timeout(Duration::from_millis(15000))?;
    // Ensure WAL mode and minimal locking footprint
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA query_only=ON;")?;
    Ok(conn)
}

/// Check if an error is a "database is locked/busy" error worth retrying.
fn is_locked_error(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(ffi_err, _) => matches!(
            ffi_err.code,
            rusqlite::ffi::ErrorCode::DatabaseBusy | rusqlite::ffi::ErrorCode::DatabaseLocked
        ),
        _ => false,
    }
}

/// Execute a query with retry-on-locked using jittered exponential backoff.
fn with_retry<T, F>(f: F) -> Result<T>
where
    F: Fn() -> std::result::Result<T, rusqlite::Error>,
{
    let max_retries = db_max_retries();
    for attempt in 0..max_retries {
        match f() {
            Ok(val) => return Ok(val),
            Err(e) if is_locked_error(&e) && attempt < max_retries - 1 => {
                thread::sleep(retry_delay_with_jitter(attempt));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(OrchestratorError::Config(
        "db max retries exhausted without result".into(),
    ))
}

/// Get the status and assignee for a bead.
pub fn bead_state(
    conn: &Connection,
    bead_id: &str,
) -> Result<Option<(BeadStatus, Option<String>)>> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(
            "SELECT coalesce(status,''), coalesce(assignee,'') FROM issues WHERE id = ?1 LIMIT 1",
        )?;
        let result = stmt.query_row([bead_id], |row| {
            let status_str: String = row.get(0)?;
            let assignee_str: String = row.get(1)?;
            Ok((status_str, assignee_str))
        });

        match result {
            Ok((status_str, assignee_str)) => {
                let status = BeadStatus::from_str(&status_str).unwrap_or(BeadStatus::Open);
                let assignee = if assignee_str.is_empty() {
                    None
                } else {
                    Some(assignee_str)
                };
                Ok(Some((status, assignee)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    })
}

/// Count active (open or in_progress) assignments for a worker.
pub fn active_assignment_count(conn: &Connection, worker: &str) -> Result<usize> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(
            "SELECT count(*) FROM issues WHERE status IN ('open','in_progress') AND assignee = ?1",
        )?;
        let count: i64 = stmt.query_row([worker], |row| row.get(0))?;
        Ok(count as usize)
    })
}

/// Get the highest-priority active bead assigned to a worker.
pub fn assigned_bead_for_worker(conn: &Connection, worker: &str) -> Result<Option<BeadInfo>> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(
            "SELECT id, coalesce(title,''), coalesce(description,''), status, \
             coalesce(assignee,''), updated_at, coalesce(priority,999) \
             FROM issues \
             WHERE status IN ('open','in_progress') AND assignee = ?1 \
             ORDER BY priority, created_at LIMIT 1",
        )?;
        let result = stmt.query_row([worker], |row| {
            let status_str: String = row.get(3)?;
            let assignee_str: String = row.get(4)?;
            Ok(BeadInfo {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                status: BeadStatus::from_str(&status_str).unwrap_or(BeadStatus::Open),
                assignee: if assignee_str.is_empty() {
                    None
                } else {
                    Some(assignee_str)
                },
                updated_at: row.get(5)?,
                priority: row.get(6)?,
                crate_labels: Vec::new(),
                effort: None,
                unblock_impact: 0,
            })
        });

        match result {
            Ok(bead) => Ok(Some(bead)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    })
}

/// Count beads with a given status.
pub fn count_by_status(conn: &Connection, status: BeadStatus) -> Result<usize> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached("SELECT count(*) FROM issues WHERE status = ?1")?;
        let count: i64 = stmt.query_row([status.as_str()], |row| row.get(0))?;
        Ok(count as usize)
    })
}

/// Label that marks a bead as blocked on the machine environment (missing
/// toolchain, operator action needed). Assignment skips these even when the
/// dependency graph says ready.
pub const ENV_BLOCKED_LABEL: &str = "env-blocked";

/// Label set by fail-streak parking: the bead failed verification repeatedly
/// and needs operator triage. Assignment skips these until the label is
/// removed.
pub const NEEDS_ATTENTION_LABEL: &str = "needs-attention";

/// Separator used for the aggregated crate-label column (ASCII unit separator,
/// cannot appear in label names).
const LABEL_SEP: char = '\u{1F}';

/// Ready-for-assignment filter. Mirrors `br ready` semantics — open, unblocked
/// by live dependencies (`blocks`/`conditional-blocks`/`waits-for` on the
/// issue, `parent-child` from open children), not deferred, deleted, or a
/// template — validated 1:1 against `br ready --unassigned` output. On top of
/// that, two assignment-specific exclusions: epics are containers (never
/// hand one to a worker), and `env-blocked`-labeled beads need operator action
/// no worker can perform.
///
/// `defer_until` normalization: br writes ISO-8601 `T` timestamps while
/// SQLite's datetime('now') uses a space; substr+replace compares the first
/// 19 chars in a uniform format.
fn ready_unassigned_where() -> String {
    format!(
        "i.status = 'open' \
         AND (i.assignee IS NULL OR i.assignee = '') \
         AND coalesce(i.issue_type,'task') != 'epic' \
         AND i.deleted_at IS NULL \
         AND coalesce(i.is_template,0) = 0 \
         AND (i.defer_until IS NULL \
              OR replace(substr(i.defer_until,1,19),'T',' ') <= datetime('now')) \
         AND NOT EXISTS ( \
             SELECT 1 FROM dependencies d JOIN issues b ON b.id = d.depends_on_id \
             WHERE d.issue_id = i.id \
               AND d.type IN ('blocks','conditional-blocks','waits-for') \
               AND b.status != 'closed' AND b.deleted_at IS NULL) \
         AND NOT EXISTS ( \
             SELECT 1 FROM dependencies pc JOIN issues c ON c.id = pc.issue_id \
             WHERE pc.depends_on_id = i.id \
               AND pc.type = 'parent-child' \
               AND c.status != 'closed' AND c.deleted_at IS NULL) \
         AND NOT EXISTS ( \
             SELECT 1 FROM labels sl \
             WHERE sl.issue_id = i.id \
               AND sl.label IN ('{ENV_BLOCKED_LABEL}','{NEEDS_ATTENTION_LABEL}'))"
    )
}

fn ready_unassigned_sql() -> &'static str {
    static SQL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SQL.get_or_init(|| {
        format!(
            "SELECT i.id, coalesce(i.title,''), coalesce(i.description,''), i.status, \
             coalesce(i.assignee,''), i.updated_at, coalesce(i.priority,999), \
             (SELECT coalesce(group_concat(l.label, char(31)),'') FROM labels l \
              WHERE l.issue_id = i.id AND l.label LIKE 'crate:%') AS crate_labels, \
             (SELECT l.label FROM labels l \
              WHERE l.issue_id = i.id AND l.label LIKE 'effort:%' LIMIT 1) AS effort_label, \
             (SELECT count(*) FROM dependencies du JOIN issues di ON di.id = du.issue_id \
              WHERE du.depends_on_id = i.id \
                AND du.type IN ('blocks','conditional-blocks','waits-for') \
                AND di.status = 'open' AND di.deleted_at IS NULL) AS unblock_impact \
             FROM issues i \
             WHERE {} \
             ORDER BY coalesce(i.priority,999), unblock_impact DESC, i.created_at \
             LIMIT ?1",
            ready_unassigned_where()
        )
    })
}

fn count_ready_unassigned_sql() -> &'static str {
    static SQL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SQL.get_or_init(|| {
        format!(
            "SELECT count(*) FROM issues i WHERE {}",
            ready_unassigned_where()
        )
    })
}

/// Count ready (assignable) beads that have no assignee.
pub fn count_ready_unassigned(conn: &Connection) -> Result<usize> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(count_ready_unassigned_sql())?;
        let count: i64 = stmt.query_row([], |row| row.get(0))?;
        Ok(count as usize)
    })
}

/// Get ready unassigned beads: P0 first, then by how many open beads a close
/// would unblock (critical path first), then oldest.
pub fn ready_unassigned(conn: &Connection, limit: usize) -> Result<Vec<BeadInfo>> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(ready_unassigned_sql())?;
        let rows = stmt.query_map([limit as i64], |row| {
            let status_str: String = row.get(3)?;
            let assignee_str: String = row.get(4)?;
            let crate_labels_str: String = row.get(7)?;
            let effort_label: Option<String> = row.get(8)?;
            Ok(BeadInfo {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                status: BeadStatus::from_str(&status_str).unwrap_or(BeadStatus::Open),
                assignee: if assignee_str.is_empty() {
                    None
                } else {
                    Some(assignee_str)
                },
                updated_at: row.get(5)?,
                priority: row.get(6)?,
                crate_labels: if crate_labels_str.is_empty() {
                    Vec::new()
                } else {
                    crate_labels_str
                        .split(LABEL_SEP)
                        .map(str::to_string)
                        .collect()
                },
                effort: effort_label
                    .and_then(|l| l.strip_prefix("effort:").map(str::to_string))
                    .filter(|s| !s.is_empty()),
                unblock_impact: row.get(9)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
    })
}

/// Count live in_progress beads held by the given workers (close-stall
/// saturation input). Excludes soft-deleted rows and beads assigned outside
/// this swarm, so phantom rows can't fake saturation and latch backpressure.
pub fn count_in_progress_for_workers(conn: &Connection, workers: &[String]) -> Result<usize> {
    if workers.is_empty() {
        return Ok(0);
    }
    let placeholders = vec!["?"; workers.len()].join(",");
    let sql = format!(
        "SELECT count(*) FROM issues \
         WHERE status = 'in_progress' AND deleted_at IS NULL \
           AND assignee IN ({placeholders})"
    );
    with_retry(|| {
        let mut stmt = conn.prepare_cached(&sql)?;
        let n: i64 =
            stmt.query_row(rusqlite::params_from_iter(workers.iter()), |row| row.get(0))?;
        Ok(n as usize)
    })
}

/// Aggregate queue-hygiene counters for the gardener digest.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HygieneCounts {
    pub open_total: usize,
    pub open_p0: usize,
    pub open_unassigned: usize,
    /// Actually assignable (full ready semantics).
    pub truly_ready: usize,
    /// Open unassigned beads hidden behind a live blocking dependency.
    pub blocked_open: usize,
    /// Open beads untouched for longer than the stale window.
    pub stale_open: usize,
    /// Highest-impact live blockers: (bead id, open beads it blocks).
    pub top_blockers: Vec<(String, i64)>,
}

/// Collect queue-hygiene counters in one pass (gardener input).
pub fn queue_hygiene(conn: &Connection, stale_days: i64) -> Result<HygieneCounts> {
    let truly_ready = count_ready_unassigned(conn)?;
    with_retry(|| {
        let count1 = |sql: &str| -> std::result::Result<usize, rusqlite::Error> {
            let mut stmt = conn.prepare_cached(sql)?;
            let n: i64 = stmt.query_row([], |row| row.get(0))?;
            Ok(n as usize)
        };
        let open_total =
            count1("SELECT count(*) FROM issues WHERE status='open' AND deleted_at IS NULL")?;
        let open_p0 = count1(
            "SELECT count(*) FROM issues \
             WHERE status='open' AND deleted_at IS NULL AND coalesce(priority,999) = 0",
        )?;
        let open_unassigned = count1(
            "SELECT count(*) FROM issues \
             WHERE status='open' AND deleted_at IS NULL \
               AND (assignee IS NULL OR assignee = '')",
        )?;
        let blocked_open = count1(
            "SELECT count(*) FROM issues i \
             WHERE i.status='open' AND i.deleted_at IS NULL \
               AND (i.assignee IS NULL OR i.assignee = '') \
               AND EXISTS ( \
                   SELECT 1 FROM dependencies d JOIN issues b ON b.id = d.depends_on_id \
                   WHERE d.issue_id = i.id \
                     AND d.type IN ('blocks','conditional-blocks','waits-for') \
                     AND b.status != 'closed' AND b.deleted_at IS NULL)",
        )?;
        let stale_open = {
            let mut stmt = conn.prepare_cached(
                "SELECT count(*) FROM issues \
                 WHERE status='open' AND deleted_at IS NULL \
                   AND replace(substr(updated_at,1,19),'T',' ') < datetime('now', ?1)",
            )?;
            let n: i64 = stmt.query_row([format!("-{stale_days} days")], |row| row.get(0))?;
            n as usize
        };
        let top_blockers = {
            let mut stmt = conn.prepare_cached(
                "SELECT b.id, count(*) AS n \
                 FROM issues b \
                 JOIN dependencies d ON d.depends_on_id = b.id \
                 JOIN issues dep ON dep.id = d.issue_id \
                 WHERE b.status IN ('open','in_progress') AND b.deleted_at IS NULL \
                   AND d.type IN ('blocks','conditional-blocks','waits-for') \
                   AND dep.status = 'open' AND dep.deleted_at IS NULL \
                 GROUP BY b.id ORDER BY n DESC, b.id LIMIT 5",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        Ok(HygieneCounts {
            open_total,
            open_p0,
            open_unassigned,
            truly_ready,
            blocked_open,
            stale_open,
            top_blockers,
        })
    })
}

/// Distinct `crate:*` labels attached to in_progress beads — the crates the
/// swarm is currently editing. Input for collision-aware assignment.
pub fn busy_crate_labels(conn: &Connection) -> Result<std::collections::HashSet<String>> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(
            "SELECT DISTINCT l.label FROM labels l \
             JOIN issues i ON i.id = l.issue_id \
             WHERE i.status = 'in_progress' AND i.deleted_at IS NULL \
               AND l.label LIKE 'crate:%'",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<std::collections::HashSet<_>, _>>()
    })
}

/// Get all in_progress beads assigned to workers not in the active set.
/// Returns (bead_id, assignee) pairs for stale assignments.
pub fn stale_assignments(
    conn: &Connection,
    active_workers: &[String],
) -> Result<Vec<(String, String)>> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(
            "SELECT id, assignee FROM issues \
             WHERE status IN ('open','in_progress') \
             AND assignee IS NOT NULL AND assignee != ''",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let assignee: String = row.get(1)?;
            Ok((id, assignee))
        })?;
        let all: Vec<(String, String)> = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(all
            .into_iter()
            .filter(|(_, assignee)| !active_workers.contains(assignee))
            .collect())
    })
}

/// Find beads stuck `in_progress` with no assignee (orphaned).
///
/// These arise when a close succeeds but the follow-up reassign fails, or an
/// assignment is half-applied — the bead is owned by nobody and no worker will
/// ever complete it. Returns their IDs so the coordinator can reopen them.
pub fn orphaned_in_progress(conn: &Connection) -> Result<Vec<String>> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(
            "SELECT id FROM issues \
             WHERE status = 'in_progress' \
             AND (assignee IS NULL OR assignee = '')",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
    })
}

/// Find workers with more than one in_progress assignment.
/// Returns (assignee, bead_id) pairs for the extra assignments (keeping the highest-priority one).
pub fn extra_assignments(conn: &Connection) -> Result<Vec<(String, String)>> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(
            "WITH ranked AS ( \
               SELECT id, assignee, priority, created_at, \
                 row_number() OVER ( \
                   PARTITION BY assignee \
                   ORDER BY priority ASC, created_at DESC, id DESC \
                 ) AS rn \
               FROM issues \
               WHERE status = 'in_progress' \
                 AND assignee IS NOT NULL AND assignee != '' \
             ) \
             SELECT assignee, id FROM ranked WHERE rn > 1 ORDER BY assignee, rn",
        )?;
        let rows = stmt.query_map([], |row| {
            let assignee: String = row.get(0)?;
            let id: String = row.get(1)?;
            Ok((assignee, id))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
    })
}

/// Get active assignments for a worker, optionally excluding one bead.
pub fn worker_active_assignments(
    conn: &Connection,
    worker: &str,
    exclude_bead: Option<&str>,
) -> Result<Vec<String>> {
    with_retry(|| {
        let ids = if let Some(exclude) = exclude_bead {
            let mut stmt = conn.prepare_cached(
                "SELECT id FROM issues \
                 WHERE status = 'in_progress' AND assignee = ?1 AND id != ?2 \
                 ORDER BY priority, created_at",
            )?;
            let rows = stmt.query_map(rusqlite::params![worker, exclude], |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let mut stmt = conn.prepare_cached(
                "SELECT id FROM issues \
                 WHERE status = 'in_progress' AND assignee = ?1 \
                 ORDER BY priority, created_at",
            )?;
            let rows = stmt.query_map([worker], |row| row.get::<_, String>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        Ok(ids)
    })
}

/// Read the description field for a bead. Returns None if the bead doesn't exist.
pub fn bead_description(conn: &Connection, bead_id: &str) -> Result<Option<String>> {
    with_retry(|| {
        let mut stmt = conn
            .prepare_cached("SELECT coalesce(description,'') FROM issues WHERE id = ?1 LIMIT 1")?;
        let result = stmt.query_row([bead_id], |row| {
            let desc: String = row.get(0)?;
            Ok(desc)
        });

        match result {
            Ok(desc) => {
                if desc.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(desc))
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    })
}

/// `crate:*` labels attached to one bead — used to scope the stale-recovery
/// fallback check to the crates the bead actually touched instead of a
/// full-workspace build.
pub fn bead_crate_labels(conn: &Connection, bead_id: &str) -> Result<Vec<String>> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(
            "SELECT label FROM labels WHERE issue_id = ?1 AND label LIKE 'crate:%'",
        )?;
        let rows = stmt.query_map([bead_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
    })
}

/// Collect bead titles matching any of the given statuses.
pub fn bead_titles_by_status(conn: &Connection, statuses: &[BeadStatus]) -> Result<Vec<String>> {
    if statuses.is_empty() {
        return Ok(vec![]);
    }
    with_retry(|| {
        let placeholders: Vec<&str> = statuses.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT coalesce(title,'') FROM issues WHERE status IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let title: String = row.get(0)?;
            Ok(title)
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
    })
}

/// Collect all bead titles regardless of status (for comprehensive dedup).
pub fn bead_titles_all(conn: &Connection) -> Result<Vec<String>> {
    with_retry(|| {
        let mut stmt = conn.prepare("SELECT coalesce(title,'') FROM issues")?;
        let rows = stmt.query_map([], |row| {
            let title: String = row.get(0)?;
            Ok(title)
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
    })
}

/// Find bead descriptions containing the given pattern (all statuses).
pub fn bead_descriptions_containing(conn: &Connection, pattern: &str) -> Result<Vec<String>> {
    with_retry(|| {
        let mut stmt =
            conn.prepare("SELECT coalesce(description,'') FROM issues WHERE description LIKE ?1")?;
        let like_pattern = format!("%{}%", pattern);
        let rows = stmt.query_map([&like_pattern], |row| {
            let desc: String = row.get(0)?;
            Ok(desc)
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
    })
}

/// Find bead descriptions containing the given pattern, filtered by status.
pub fn bead_descriptions_containing_by_status(
    conn: &Connection,
    pattern: &str,
    statuses: &[BeadStatus],
) -> Result<Vec<String>> {
    if statuses.is_empty() {
        return Ok(vec![]);
    }
    with_retry(|| {
        let placeholders: Vec<&str> = statuses.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT coalesce(description,'') FROM issues WHERE description LIKE ?1 AND status IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let like_pattern = format!("%{}%", pattern);
        let status_strs: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
        // Build params: first is the LIKE pattern, rest are status strings
        let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(1 + statuses.len());
        params.push(&like_pattern);
        for s in &status_strs {
            params.push(s);
        }
        let rows = stmt.query_map(params.as_slice(), |row| {
            let desc: String = row.get(0)?;
            Ok(desc)
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
    })
}

/// Find the first bead ID whose description contains the given pattern,
/// restricted to the specified statuses. Returns None if no match.
///
/// Used to detect "closed bead exists for this planner-key" so the
/// planner auto-create path can reactivate instead of creating a duplicate.
pub fn bead_id_by_description_and_status(
    conn: &Connection,
    pattern: &str,
    statuses: &[BeadStatus],
) -> Result<Option<String>> {
    if statuses.is_empty() {
        return Ok(None);
    }
    with_retry(|| {
        let placeholders: Vec<&str> = statuses.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT id FROM issues WHERE description LIKE ?1 AND status IN ({}) LIMIT 1",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let like_pattern = format!("%{}%", pattern);
        let status_strs: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
        let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(1 + statuses.len());
        params.push(&like_pattern);
        for s in &status_strs {
            params.push(s);
        }
        let mut rows = stmt.query(params.as_slice())?;
        if let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            Ok(Some(id))
        } else {
            Ok(None)
        }
    })
}

/// Return a map of planner-key -> bead id for all beads whose description
/// contains a `[planner-key: <key>]` marker and whose status is in
/// `statuses`.
///
/// Used to populate an in-memory index so the planner auto-create loop
/// can detect "a bead with this planner-key already exists" without
/// querying SQLite per-record.
pub fn planner_key_to_bead_id_by_status(
    conn: &Connection,
    statuses: &[BeadStatus],
) -> Result<std::collections::HashMap<String, String>> {
    if statuses.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    with_retry(|| {
        let placeholders: Vec<&str> = statuses.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT id, coalesce(description,'') FROM issues \
             WHERE description LIKE ?1 AND status IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let like_pattern = "%[planner-key: %".to_string();
        let status_strs: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
        let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(1 + statuses.len());
        params.push(&like_pattern);
        for s in &status_strs {
            params.push(s);
        }
        let rows = stmt.query_map(params.as_slice(), |row| {
            let id: String = row.get(0)?;
            let desc: String = row.get(1)?;
            Ok((id, desc))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (id, desc) = row?;
            if let Some(key) = extract_planner_key(&desc) {
                map.entry(key).or_insert(id);
            }
        }
        Ok(map)
    })
}

/// Extract the `<key>` value from the first `[planner-key: <key>]` marker
/// found in `desc`, trimming whitespace. Returns None if no marker is
/// present.
fn extract_planner_key(desc: &str) -> Option<String> {
    let start = desc.find("[planner-key:")?;
    let after = &desc[start + "[planner-key:".len()..];
    let end = after.find(']')?;
    let key = after[..end].trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE issues (
                id TEXT PRIMARY KEY,
                title TEXT,
                description TEXT,
                status TEXT,
                assignee TEXT,
                priority INTEGER DEFAULT 999,
                issue_type TEXT DEFAULT 'task',
                defer_until DATETIME,
                deleted_at DATETIME,
                is_template INTEGER DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE dependencies (
                issue_id TEXT NOT NULL,
                depends_on_id TEXT NOT NULL,
                type TEXT NOT NULL DEFAULT 'blocks',
                PRIMARY KEY (issue_id, depends_on_id)
            );
            CREATE TABLE labels (
                issue_id TEXT NOT NULL,
                label TEXT NOT NULL,
                PRIMARY KEY (issue_id, label)
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_bead_state_found() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO issues (id, title, status, assignee) VALUES (?1, ?2, ?3, ?4)",
            ["pat-abc", "Test bead", "in_progress", "WorkerA"],
        )
        .unwrap();

        let state = bead_state(&conn, "pat-abc").unwrap();
        assert_eq!(
            state,
            Some((BeadStatus::InProgress, Some("WorkerA".to_string())))
        );
    }

    #[test]
    fn test_bead_state_not_found() {
        let conn = setup_test_db();
        let state = bead_state(&conn, "pat-nonexistent").unwrap();
        assert_eq!(state, None);
    }

    /// Only the bead's own `crate:*` labels come back — other labels and other
    /// beads' labels don't. Missing beads yield an empty list, not an error.
    #[test]
    fn test_bead_crate_labels() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status) VALUES ('pat-1', 'B1', 'in_progress');
             INSERT INTO labels (issue_id, label) VALUES
             ('pat-1', 'crate:gdscene'),
             ('pat-1', 'crate:gdrender2d'),
             ('pat-1', 'needs-attention'),
             ('pat-2', 'crate:gdeditor');",
        )
        .unwrap();

        let mut labels = bead_crate_labels(&conn, "pat-1").unwrap();
        labels.sort();
        assert_eq!(labels, vec!["crate:gdrender2d", "crate:gdscene"]);
        assert!(bead_crate_labels(&conn, "pat-missing").unwrap().is_empty());
    }

    #[test]
    fn test_active_assignment_count() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, assignee) VALUES
             ('pat-1', 'B1', 'in_progress', 'WorkerA'),
             ('pat-2', 'B2', 'in_progress', 'WorkerA'),
             ('pat-3', 'B3', 'closed', 'WorkerA'),
             ('pat-4', 'B4', 'in_progress', 'WorkerB');",
        )
        .unwrap();

        assert_eq!(active_assignment_count(&conn, "WorkerA").unwrap(), 2);
        assert_eq!(active_assignment_count(&conn, "WorkerB").unwrap(), 1);
        assert_eq!(active_assignment_count(&conn, "WorkerC").unwrap(), 0);
    }

    #[test]
    fn test_assigned_bead_for_worker() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, assignee, priority) VALUES
             ('pat-1', 'Low prio', 'in_progress', 'WorkerA', 5),
             ('pat-2', 'High prio', 'in_progress', 'WorkerA', 1);",
        )
        .unwrap();

        let bead = assigned_bead_for_worker(&conn, "WorkerA").unwrap().unwrap();
        assert_eq!(bead.id, "pat-2"); // highest priority (lowest number)
    }

    #[test]
    fn test_ready_unassigned() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, assignee, priority) VALUES
             ('pat-1', 'Assigned', 'open', 'WorkerA', 1),
             ('pat-2', 'Ready', 'open', '', 2),
             ('pat-3', 'Ready2', 'open', NULL, 1),
             ('pat-4', 'Closed', 'closed', '', 1);",
        )
        .unwrap();

        let beads = ready_unassigned(&conn, 10).unwrap();
        assert_eq!(beads.len(), 2);
        assert_eq!(beads[0].id, "pat-3"); // higher priority
        assert_eq!(beads[1].id, "pat-2");
    }

    /// An `effort:<level>` label surfaces on BeadInfo.effort as the level only
    /// (prefix stripped); an unlabeled bead reports None.
    #[test]
    fn test_ready_unassigned_reads_effort_label() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, assignee, priority) VALUES
             ('pat-hi', 'Complex', 'open', '', 1),
             ('pat-plain', 'Simple', 'open', '', 2);
             INSERT INTO labels (issue_id, label) VALUES
             ('pat-hi', 'effort:xhigh'),
             ('pat-hi', 'crate:gdscene');",
        )
        .unwrap();

        let beads = ready_unassigned(&conn, 10).unwrap();
        let hi = beads.iter().find(|b| b.id == "pat-hi").unwrap();
        let plain = beads.iter().find(|b| b.id == "pat-plain").unwrap();
        assert_eq!(hi.effort.as_deref(), Some("xhigh"), "level only, prefix stripped");
        assert_eq!(plain.effort, None, "unlabeled bead has no effort");
    }

    /// Beads with a live blocker (any blocking dep type) are not ready;
    /// closing the blocker makes them ready. Non-blocking dep types don't hide.
    #[test]
    fn test_ready_excludes_dep_blocked_until_blocker_closes() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, priority) VALUES
             ('pat-blocker', 'Blocker', 'in_progress', 0),
             ('pat-blocks', 'Blocked via blocks', 'open', 0),
             ('pat-waits', 'Blocked via waits-for', 'open', 0),
             ('pat-cond', 'Blocked via conditional-blocks', 'open', 0),
             ('pat-related', 'Related only', 'open', 0);
             INSERT INTO dependencies (issue_id, depends_on_id, type) VALUES
             ('pat-blocks', 'pat-blocker', 'blocks'),
             ('pat-waits', 'pat-blocker', 'waits-for'),
             ('pat-cond', 'pat-blocker', 'conditional-blocks'),
             ('pat-related', 'pat-blocker', 'related');",
        )
        .unwrap();

        let ids: Vec<String> = ready_unassigned(&conn, 10)
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(ids, vec!["pat-related"], "only non-blocking dep is ready");
        assert_eq!(count_ready_unassigned(&conn).unwrap(), 1);

        conn.execute(
            "UPDATE issues SET status='closed' WHERE id='pat-blocker'",
            [],
        )
        .unwrap();
        assert_eq!(
            count_ready_unassigned(&conn).unwrap(),
            4,
            "closing the blocker frees all dependents"
        );
    }

    /// Epics are never assignable, and a non-epic parent with an open child
    /// (parent-child dep rows point child -> parent) is blocked until the
    /// child closes.
    #[test]
    fn test_ready_excludes_epics_and_open_parents() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, priority, issue_type) VALUES
             ('pat-epic', 'Unblocked epic', 'open', 0, 'epic'),
             ('pat-parent', 'Task with subtask', 'open', 0, 'task'),
             ('pat-child', 'Subtask', 'open', 1, 'task');
             INSERT INTO dependencies (issue_id, depends_on_id, type) VALUES
             ('pat-child', 'pat-parent', 'parent-child');",
        )
        .unwrap();

        let ids: Vec<String> = ready_unassigned(&conn, 10)
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect();
        // Epic hidden even with no deps; parent hidden while child open.
        assert_eq!(ids, vec!["pat-child"]);

        conn.execute("UPDATE issues SET status='closed' WHERE id='pat-child'", [])
            .unwrap();
        let ids: Vec<String> = ready_unassigned(&conn, 10)
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(
            ids,
            vec!["pat-parent"],
            "epic still hidden after children close"
        );
    }

    /// Deferred (future defer_until, ISO-T format), deleted, template, and
    /// env-blocked-labeled beads are not assignable; past defer_until is.
    #[test]
    fn test_ready_excludes_deferred_deleted_template_env_blocked() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, priority, defer_until, deleted_at, is_template) VALUES
             ('pat-deferred', 'Deferred', 'open', 0, '2099-01-01T00:00:00Z', NULL, 0),
             ('pat-undeferred', 'Defer elapsed', 'open', 0, '2020-01-01T00:00:00Z', NULL, 0),
             ('pat-deleted', 'Deleted', 'open', 0, NULL, '2026-01-01T00:00:00Z', 0),
             ('pat-template', 'Template', 'open', 0, NULL, NULL, 1),
             ('pat-envblocked', 'Env blocked', 'open', 0, NULL, NULL, 0),
             ('pat-parked', 'Parked after fail streak', 'open', 0, NULL, NULL, 0);
             INSERT INTO labels (issue_id, label) VALUES
             ('pat-envblocked', 'env-blocked'),
             ('pat-parked', 'needs-attention');",
        )
        .unwrap();
        // Same-day ISO-T defer that elapsed an hour ago: without the
        // T→space normalization ('T' > ' ' in ASCII) this row would stay
        // hidden until midnight — this pins the normalization.
        conn.execute(
            "INSERT INTO issues (id, title, status, priority, defer_until) VALUES
             ('pat-defer-today', 'Defer elapsed today', 'open', 0,
              strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now','-1 hour')))",
            [],
        )
        .unwrap();

        let mut ids: Vec<String> = ready_unassigned(&conn, 10)
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["pat-defer-today", "pat-undeferred"]);
        assert_eq!(count_ready_unassigned(&conn).unwrap(), 2);
    }

    /// Saturation counting must ignore soft-deleted rows and beads assigned
    /// to agents outside the current worker set (phantom saturation would
    /// latch close-stall backpressure on an under-saturated swarm).
    #[test]
    fn test_count_in_progress_for_workers_scopes_and_skips_deleted() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, assignee, deleted_at) VALUES
             ('pat-1', 'Mine', 'in_progress', 'WorkerA', NULL),
             ('pat-2', 'Mine too', 'in_progress', 'WorkerB', NULL),
             ('pat-3', 'Deleted phantom', 'in_progress', 'WorkerA', '2026-01-01T00:00:00Z'),
             ('pat-4', 'Foreign session', 'in_progress', 'SomeoneElse', NULL),
             ('pat-5', 'Open not in progress', 'open', 'WorkerA', NULL);",
        )
        .unwrap();
        let workers = vec!["WorkerA".to_string(), "WorkerB".to_string()];
        assert_eq!(count_in_progress_for_workers(&conn, &workers).unwrap(), 2);
        assert_eq!(count_in_progress_for_workers(&conn, &[]).unwrap(), 0);
    }

    /// Ordering: priority first, then unblock impact (open dependents) desc,
    /// then created_at. A P1 that unblocks nothing still loses to every P0.
    #[test]
    fn test_ready_orders_by_priority_then_unblock_impact_then_age() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, priority, created_at) VALUES
             ('pat-p0-old', 'P0 no impact, oldest', 'open', 0, '2026-01-01T00:00:00Z'),
             ('pat-p0-impact', 'P0 unblocks two', 'open', 0, '2026-01-03T00:00:00Z'),
             ('pat-p0-new', 'P0 no impact, newest', 'open', 0, '2026-01-02T00:00:00Z'),
             ('pat-p1-impact', 'P1 unblocks one', 'open', 1, '2026-01-01T00:00:00Z'),
             ('pat-dep1', 'Dependent 1', 'open', 2, '2026-01-04T00:00:00Z'),
             ('pat-dep2', 'Dependent 2', 'open', 2, '2026-01-04T00:00:00Z'),
             ('pat-dep3', 'Dependent 3', 'open', 2, '2026-01-04T00:00:00Z');
             INSERT INTO dependencies (issue_id, depends_on_id, type) VALUES
             ('pat-dep1', 'pat-p0-impact', 'blocks'),
             ('pat-dep2', 'pat-p0-impact', 'waits-for'),
             ('pat-dep3', 'pat-p1-impact', 'blocks');",
        )
        .unwrap();

        let beads = ready_unassigned(&conn, 10).unwrap();
        let ids: Vec<&str> = beads.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["pat-p0-impact", "pat-p0-old", "pat-p0-new", "pat-p1-impact"],
            "impact wins within priority; age breaks impact ties; priority trumps impact"
        );
        assert_eq!(beads[0].unblock_impact, 2);
        assert_eq!(beads[3].unblock_impact, 1);
    }

    /// queue_hygiene counts the gardener's inputs: dilution, blockage,
    /// staleness, and highest-impact blockers.
    #[test]
    fn test_queue_hygiene_counts() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, priority, updated_at) VALUES
             ('pat-p0a', 'P0 ready', 'open', 0, '2026-07-13T00:00:00Z'),
             ('pat-p0b', 'P0 blocked', 'open', 0, '2026-07-13T00:00:00Z'),
             ('pat-p1', 'P1 stale', 'open', 1, '2020-01-01T00:00:00Z'),
             ('pat-blocker', 'Blocker in progress', 'in_progress', 0, '2026-07-13T00:00:00Z'),
             ('pat-closed', 'Closed', 'closed', 0, '2020-01-01T00:00:00Z');
             INSERT INTO dependencies (issue_id, depends_on_id, type) VALUES
             ('pat-p0b', 'pat-blocker', 'blocks'),
             ('pat-p1', 'pat-blocker', 'blocks');",
        )
        .unwrap();

        let h = queue_hygiene(&conn, 14).unwrap();
        assert_eq!(h.open_total, 3);
        assert_eq!(h.open_p0, 2);
        assert_eq!(h.open_unassigned, 3);
        assert_eq!(h.truly_ready, 1, "only pat-p0a is assignable");
        assert_eq!(h.blocked_open, 2);
        assert_eq!(h.stale_open, 1, "pat-p1 untouched since 2020");
        assert_eq!(h.top_blockers, vec![("pat-blocker".to_string(), 2)]);
    }

    /// crate:* labels round-trip into BeadInfo.crate_labels; busy_crate_labels
    /// reports only in_progress beads' crate labels.
    #[test]
    fn test_crate_labels_and_busy_set() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, priority) VALUES
             ('pat-ready', 'Ready with labels', 'open', 0),
             ('pat-busy', 'In progress', 'in_progress', 0),
             ('pat-done', 'Closed', 'closed', 0);
             INSERT INTO labels (issue_id, label) VALUES
             ('pat-ready', 'crate:gdrender2d'),
             ('pat-ready', 'crate:gdscene'),
             ('pat-ready', 'area:web'),
             ('pat-busy', 'crate:gdeditor'),
             ('pat-done', 'crate:gdaudio');",
        )
        .unwrap();

        let beads = ready_unassigned(&conn, 10).unwrap();
        assert_eq!(beads.len(), 1);
        let mut labels = beads[0].crate_labels.clone();
        labels.sort();
        assert_eq!(labels, vec!["crate:gdrender2d", "crate:gdscene"]);

        let busy = busy_crate_labels(&conn).unwrap();
        assert!(busy.contains("crate:gdeditor"));
        assert!(!busy.contains("crate:gdaudio"), "closed beads are not busy");
        assert!(
            !busy.contains("crate:gdrender2d"),
            "open beads are not busy"
        );
    }

    #[test]
    fn test_extra_assignments() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, assignee, priority) VALUES
             ('pat-1', 'B1', 'in_progress', 'WorkerA', 1),
             ('pat-2', 'B2', 'in_progress', 'WorkerA', 2),
             ('pat-3', 'B3', 'in_progress', 'WorkerB', 1);",
        )
        .unwrap();

        let extras = extra_assignments(&conn).unwrap();
        assert_eq!(extras.len(), 1);
        assert_eq!(extras[0].0, "WorkerA");
        // pat-2 is the extra (lower priority = higher number)
        assert_eq!(extras[0].1, "pat-2");
    }

    #[test]
    fn test_stale_assignments() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, status, assignee) VALUES
             ('pat-1', 'B1', 'in_progress', 'WorkerA'),
             ('pat-2', 'B2', 'in_progress', 'WorkerB'),
             ('pat-3', 'B3', 'in_progress', 'WorkerC');",
        )
        .unwrap();

        let active = vec!["WorkerA".to_string(), "WorkerC".to_string()];
        let stale = stale_assignments(&conn, &active).unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0], ("pat-2".to_string(), "WorkerB".to_string()));
    }

    /// Verify retry_delay_with_jitter produces increasing delays with bounded jitter.
    #[test]
    fn test_retry_delay_with_jitter_increases() {
        // Collect delays for attempts 0..4 — each should be >= the base for that attempt
        let base_ms = db_retry_base_delay_ms();
        for attempt in 0..4u32 {
            let delay = retry_delay_with_jitter(attempt);
            let expected_min = Duration::from_millis(base_ms * (1u64 << attempt.min(3)));
            let expected_max = expected_min + Duration::from_millis(500);
            assert!(
                delay >= expected_min,
                "attempt {attempt}: delay {delay:?} < min {expected_min:?}"
            );
            assert!(
                delay <= expected_max,
                "attempt {attempt}: delay {delay:?} > max {expected_max:?}"
            );
        }
    }

    /// Verify backoff caps at 8× base (attempt 3+).
    #[test]
    fn test_retry_delay_backoff_cap() {
        let base_ms = db_retry_base_delay_ms();
        // attempt=3 and attempt=10 should both cap at 8× base
        let d3 = retry_delay_with_jitter(3);
        let d10 = retry_delay_with_jitter(10);
        let cap_min = Duration::from_millis(base_ms * 8);
        let cap_max = cap_min + Duration::from_millis(500);
        assert!(
            d3 >= cap_min && d3 <= cap_max,
            "attempt 3 not capped: {d3:?}"
        );
        assert!(
            d10 >= cap_min && d10 <= cap_max,
            "attempt 10 not capped: {d10:?}"
        );
    }

    /// with_retry succeeds immediately when the closure succeeds.
    #[test]
    fn test_with_retry_immediate_success() {
        let call_count = std::cell::Cell::new(0u32);
        let result: Result<i32> = with_retry(|| {
            call_count.set(call_count.get() + 1);
            Ok(42)
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count.get(), 1);
    }

    /// with_retry retries on DatabaseBusy and eventually succeeds.
    #[test]
    fn test_with_retry_succeeds_after_busy() {
        let call_count = std::cell::Cell::new(0u32);
        let result: Result<i32> = with_retry(|| {
            let n = call_count.get();
            call_count.set(n + 1);
            if n < 2 {
                Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                    Some("database is busy".to_string()),
                ))
            } else {
                Ok(99)
            }
        });
        assert_eq!(result.unwrap(), 99);
        assert_eq!(call_count.get(), 3); // 2 failures + 1 success
    }

    /// with_retry gives up after max retries on persistent busy errors.
    #[test]
    fn test_with_retry_exhausts_retries() {
        let call_count = std::cell::Cell::new(0u32);
        let result: Result<i32> = with_retry(|| {
            call_count.set(call_count.get() + 1);
            Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                Some("database is busy".to_string()),
            ))
        });
        assert!(result.is_err());
        assert_eq!(call_count.get(), db_max_retries());
    }

    /// with_retry does NOT retry on non-busy errors (e.g. constraint violation).
    #[test]
    fn test_with_retry_no_retry_on_other_errors() {
        let call_count = std::cell::Cell::new(0u32);
        let result: Result<i32> = with_retry(|| {
            call_count.set(call_count.get() + 1);
            Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                Some("constraint failed".to_string()),
            ))
        });
        assert!(result.is_err());
        assert_eq!(call_count.get(), 1); // no retries
    }

    /// is_locked_error correctly identifies BUSY and LOCKED codes.
    #[test]
    fn test_is_locked_error_detection() {
        let busy = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        );
        let locked = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_LOCKED),
            None,
        );
        let constraint = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
            None,
        );
        let query_err = rusqlite::Error::QueryReturnedNoRows;

        assert!(is_locked_error(&busy), "SQLITE_BUSY should be locked");
        assert!(is_locked_error(&locked), "SQLITE_LOCKED should be locked");
        assert!(
            !is_locked_error(&constraint),
            "SQLITE_CONSTRAINT should not be locked"
        );
        assert!(
            !is_locked_error(&query_err),
            "QueryReturnedNoRows should not be locked"
        );
    }

    /// open() creates a WAL-mode, query-only connection with 15s busy timeout.
    #[test]
    fn test_open_connection_wal_query_only() {
        let dir = tempfile::tempdir().unwrap();
        let beads_dir = dir.path().join(".beads");
        std::fs::create_dir_all(&beads_dir).unwrap();
        let db_path = beads_dir.join("beads.db");

        // Create the DB with the issues table
        let setup = Connection::open(&db_path).unwrap();
        setup
            .execute_batch(
                "CREATE TABLE issues (
                    id TEXT PRIMARY KEY,
                    status TEXT DEFAULT 'open',
                    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
                );",
            )
            .unwrap();
        drop(setup);

        let conn = open(dir.path()).unwrap();

        // Verify WAL mode
        let journal: String = conn
            .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "wal");

        // Verify query_only — writes should fail
        let write_result = conn.execute(
            "INSERT INTO issues (id, status) VALUES ('test', 'open')",
            [],
        );
        assert!(
            write_result.is_err(),
            "query_only connection should reject writes"
        );
    }

    /// Concurrent readers and a writer don't deadlock with WAL mode.
    /// This simulates the bv (reader) + orchestrator (writer) contention.
    #[test]
    fn test_concurrent_read_write_wal_no_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let beads_dir = dir.path().join(".beads");
        std::fs::create_dir_all(&beads_dir).unwrap();
        let db_path = beads_dir.join("beads.db");

        // Setup DB in WAL mode
        let setup = Connection::open(&db_path).unwrap();
        setup
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE issues (
                     id TEXT PRIMARY KEY,
                     status TEXT DEFAULT 'open',
                     assignee TEXT,
                     updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
                 );
                 INSERT INTO issues (id, status) VALUES ('pat-1', 'open');",
            )
            .unwrap();
        drop(setup);

        let db_path_clone = db_path.clone();
        // Spawn a reader thread that holds a connection open
        let reader = std::thread::spawn(move || {
            let conn = Connection::open(&db_path_clone).unwrap();
            conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
            conn.busy_timeout(Duration::from_millis(15000)).unwrap();

            // Start a read transaction and hold it
            let mut stmt = conn.prepare("SELECT id, status FROM issues").unwrap();
            let rows: Vec<(String, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(rows.len(), 1);

            // Hold connection open briefly to overlap with writer
            std::thread::sleep(Duration::from_millis(50));
            drop(stmt);
            drop(conn);
        });

        // Small delay so reader is active, then write
        std::thread::sleep(Duration::from_millis(10));
        let writer_conn = Connection::open(&db_path).unwrap();
        writer_conn
            .execute_batch("PRAGMA journal_mode=WAL;")
            .unwrap();
        writer_conn
            .busy_timeout(Duration::from_millis(30000))
            .unwrap();

        // This should succeed without deadlock in WAL mode
        writer_conn
            .execute(
                "UPDATE issues SET status = 'in_progress', assignee = 'WorkerA' WHERE id = 'pat-1'",
                [],
            )
            .unwrap();

        reader.join().unwrap();

        // Verify the write landed
        let verify = Connection::open(&db_path).unwrap();
        let status: String = verify
            .query_row("SELECT status FROM issues WHERE id = 'pat-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status, "in_progress");
    }

    /// Multiple concurrent writers complete without errors under WAL mode.
    #[test]
    fn test_concurrent_writers_complete() {
        let dir = tempfile::tempdir().unwrap();
        let beads_dir = dir.path().join(".beads");
        std::fs::create_dir_all(&beads_dir).unwrap();
        let db_path = beads_dir.join("beads.db");

        let setup = Connection::open(&db_path).unwrap();
        setup
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE issues (
                     id TEXT PRIMARY KEY,
                     status TEXT DEFAULT 'open',
                     assignee TEXT,
                     priority INTEGER DEFAULT 999,
                     updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
                 );",
            )
            .unwrap();
        // Insert 20 beads
        for i in 0..20 {
            setup
                .execute(
                    "INSERT INTO issues (id, status, priority) VALUES (?1, 'open', ?2)",
                    rusqlite::params![format!("pat-{i}"), i],
                )
                .unwrap();
        }
        drop(setup);

        // Spawn 4 writer threads, each claiming 5 beads
        let mut handles = vec![];
        for worker_idx in 0..4 {
            let path = db_path.clone();
            handles.push(std::thread::spawn(move || {
                for bead_idx in 0..5 {
                    let bead_id = format!("pat-{}", worker_idx * 5 + bead_idx);
                    let worker = format!("Worker{worker_idx}");
                    let conn = Connection::open(&path).unwrap();
                    conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
                    conn.busy_timeout(Duration::from_millis(30000)).unwrap();
                    conn.execute(
                        "UPDATE issues SET status = 'in_progress', assignee = ?1 WHERE id = ?2",
                        rusqlite::params![worker, bead_id],
                    )
                    .unwrap();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Verify all 20 beads were assigned
        let verify = Connection::open(&db_path).unwrap();
        let assigned: i64 = verify
            .query_row(
                "SELECT count(*) FROM issues WHERE status = 'in_progress'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assigned, 20);
    }

    /// with_retry retries on SQLITE_LOCKED (not just SQLITE_BUSY).
    #[test]
    fn test_with_retry_retries_on_locked() {
        let call_count = std::cell::Cell::new(0u32);
        let result: Result<i32> = with_retry(|| {
            let n = call_count.get();
            call_count.set(n + 1);
            if n < 1 {
                Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_LOCKED),
                    Some("database table is locked".to_string()),
                ))
            } else {
                Ok(77)
            }
        });
        assert_eq!(result.unwrap(), 77);
        assert_eq!(call_count.get(), 2); // 1 LOCKED failure + 1 success
    }

    /// with_retry fails on the last attempt boundary — exactly max_retries calls.
    #[test]
    fn test_with_retry_boundary_last_attempt_busy() {
        let max = db_max_retries();
        let call_count = std::cell::Cell::new(0u32);
        // Succeed on the very last attempt
        let result: Result<i32> = with_retry(|| {
            let n = call_count.get();
            call_count.set(n + 1);
            if n < max - 1 {
                Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                    None,
                ))
            } else {
                Ok(123)
            }
        });
        assert_eq!(result.unwrap(), 123);
        assert_eq!(call_count.get(), max);
    }

    /// retry_delay_with_jitter handles saturating arithmetic with a large base delay.
    #[test]
    fn test_retry_delay_saturating_large_base() {
        // Temporarily override the env var to simulate a huge base delay
        // We can't easily override the env var in a test-safe way, so instead
        // we directly test the arithmetic: u64::MAX * 8 should saturate, not panic.
        let huge: u64 = u64::MAX / 2;
        let factor: u64 = 1u64 << 3u32; // 8
        let result = huge.saturating_mul(factor);
        assert_eq!(result, u64::MAX, "saturating_mul should cap at u64::MAX");

        // And adding jitter (max 499) to u64::MAX should also saturate
        let with_jitter = u64::MAX.saturating_add(499);
        assert_eq!(with_jitter, u64::MAX);
    }

    /// Jitter is derived from clock nanos — different attempt levels produce
    /// different base delays, confirming the exponential backoff component works
    /// even when jitter happens to be constant within a fast test.
    #[test]
    fn test_retry_delay_different_attempts_differ() {
        let d0 = retry_delay_with_jitter(0);
        let d2 = retry_delay_with_jitter(2);
        // attempt=0 base is 1×, attempt=2 base is 4×, so d2 should be strictly larger
        // even with max jitter on d0 (500ms) and zero jitter on d2.
        // d0 max = 1000+499 = 1499ms, d2 min = 4000ms
        assert!(
            d2 > d0,
            "attempt=2 ({d2:?}) should be larger than attempt=0 ({d0:?})"
        );
    }

    /// db::open fails gracefully when .beads directory doesn't exist.
    #[test]
    fn test_open_missing_beads_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Don't create .beads/
        let result = open(dir.path());
        assert!(result.is_err(), "open should fail when .beads/ is missing");
    }

    /// db::open fails gracefully when .beads exists but has no .db file.
    #[test]
    fn test_open_empty_beads_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".beads")).unwrap();
        let result = open(dir.path());
        assert!(result.is_err(), "open should fail when no .db file exists");
    }

    /// bead_id_by_description_and_status returns the first matching ID
    /// restricted to the requested statuses — used to detect "closed
    /// bead exists for this planner-key" so we don't create a duplicate.
    #[test]
    fn test_bead_id_by_description_and_status_finds_closed() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, description, status) VALUES
             ('pat-open1', 'open match', 'body [planner-key: ci-export-matrix]', 'open'),
             ('pat-closed1', 'closed match', 'body [planner-key: ci-export-matrix]', 'closed'),
             ('pat-other', 'different key', 'body [planner-key: other-key]', 'closed');",
        )
        .unwrap();

        // Find only closed beads with the target planner-key.
        let id = bead_id_by_description_and_status(
            &conn,
            "[planner-key: ci-export-matrix]",
            &[BeadStatus::Closed],
        )
        .unwrap();
        assert_eq!(id.as_deref(), Some("pat-closed1"));

        // No match for a key that doesn't exist.
        let none = bead_id_by_description_and_status(
            &conn,
            "[planner-key: does-not-exist]",
            &[BeadStatus::Closed],
        )
        .unwrap();
        assert_eq!(none, None);

        // Empty status list returns None.
        let empty =
            bead_id_by_description_and_status(&conn, "[planner-key: ci-export-matrix]", &[])
                .unwrap();
        assert_eq!(empty, None);
    }

    /// planner_key_to_bead_id_by_status indexes active beads keyed by their
    /// planner-key marker so the auto-create loop can skip duplicates
    /// without a per-record query.
    #[test]
    fn test_planner_key_to_bead_id_by_status_indexes_active() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO issues (id, title, description, status) VALUES
             ('pat-open1', 'open a', 'x [planner-key: key-a] y', 'open'),
             ('pat-inprog1', 'in progress b', '[planner-key: key-b]', 'in_progress'),
             ('pat-closed1', 'closed c', '[planner-key: key-c]', 'closed'),
             ('pat-nokey', 'no key', 'no marker here', 'open');",
        )
        .unwrap();

        let active =
            planner_key_to_bead_id_by_status(&conn, &[BeadStatus::Open, BeadStatus::InProgress])
                .unwrap();
        assert_eq!(active.get("key-a").map(String::as_str), Some("pat-open1"));
        assert_eq!(active.get("key-b").map(String::as_str), Some("pat-inprog1"));
        assert!(
            !active.contains_key("key-c"),
            "closed beads must not appear in the active index"
        );
        assert_eq!(active.len(), 2);

        let closed = planner_key_to_bead_id_by_status(&conn, &[BeadStatus::Closed]).unwrap();
        assert_eq!(closed.get("key-c").map(String::as_str), Some("pat-closed1"));
        assert_eq!(closed.len(), 1);

        let empty = planner_key_to_bead_id_by_status(&conn, &[]).unwrap();
        assert!(empty.is_empty());
    }

    /// extract_planner_key must recover the key from a well-formed marker,
    /// trim surrounding whitespace, and reject every malformed variant.
    /// Regression: a broken extractor silently disables the dedupe path
    /// and the planner burst-creates duplicate beads every 10 minutes.
    #[test]
    fn test_extract_planner_key_variants() {
        // Well-formed: key found, whitespace trimmed.
        assert_eq!(
            extract_planner_key("foo [planner-key: ci-export-matrix] bar"),
            Some("ci-export-matrix".to_string())
        );
        assert_eq!(
            extract_planner_key("[planner-key:key-a]"),
            Some("key-a".to_string())
        );
        assert_eq!(
            extract_planner_key("[planner-key:   padded-key   ]"),
            Some("padded-key".to_string())
        );

        // No marker at all.
        assert_eq!(extract_planner_key("no marker here"), None);
        assert_eq!(extract_planner_key(""), None);

        // Marker present but missing closing bracket.
        assert_eq!(extract_planner_key("prefix [planner-key: oops"), None);

        // Empty key (pure whitespace) must NOT be indexed — would collide
        // every other key-less marker into a single bucket.
        assert_eq!(extract_planner_key("[planner-key:]"), None);
        assert_eq!(extract_planner_key("[planner-key:    ]"), None);

        // Multiple markers: first one wins (matches the current loop semantics).
        assert_eq!(
            extract_planner_key("[planner-key: first] then [planner-key: second]"),
            Some("first".to_string())
        );
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Jitter is always within [base*2^min(attempt,3), base*2^min(attempt,3) + 500ms].
        #[test]
        fn retry_delay_within_bounds(attempt in 0u32..20) {
            let base_ms = db_retry_base_delay_ms();
            let delay = retry_delay_with_jitter(attempt);
            let factor = 1u64 << attempt.min(3);
            let min = Duration::from_millis(base_ms.saturating_mul(factor));
            let max = min + Duration::from_millis(500);
            prop_assert!(
                delay >= min && delay <= max,
                "attempt={attempt} delay={delay:?} not in [{min:?}, {max:?}]"
            );
        }

        /// Backoff factor never exceeds 8 regardless of attempt number.
        #[test]
        fn backoff_factor_capped(attempt in 0u32..100) {
            let factor = 1u64 << attempt.min(3);
            prop_assert!(factor <= 8, "factor={factor} for attempt={attempt}");
        }
    }
}
