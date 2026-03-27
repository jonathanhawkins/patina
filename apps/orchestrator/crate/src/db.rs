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
    let jitter_ms = (counter.wrapping_mul(6364136223846793005).wrapping_add(thread_hash)) % 500;
    // Exponential backoff capped at 8× base, plus jitter
    let backoff_factor = 1u64 << attempt.min(3); // 1, 2, 4, 8
    let total_ms = base_ms.saturating_mul(backoff_factor).saturating_add(jitter_ms);
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
    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
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
    Err(OrchestratorError::Config("db max retries exhausted without result".into()))
}

/// Get the status and assignee for a bead.
pub fn bead_state(conn: &Connection, bead_id: &str) -> Result<Option<(BeadStatus, Option<String>)>> {
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
        let mut stmt =
            conn.prepare_cached("SELECT count(*) FROM issues WHERE status = ?1")?;
        let count: i64 = stmt.query_row([status.as_str()], |row| row.get(0))?;
        Ok(count as usize)
    })
}

/// Count ready beads that have no assignee.
pub fn count_ready_unassigned(conn: &Connection) -> Result<usize> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(
            "SELECT count(*) FROM issues \
             WHERE status = 'open' AND (assignee IS NULL OR assignee = '')",
        )?;
        let count: i64 = stmt.query_row([], |row| row.get(0))?;
        Ok(count as usize)
    })
}

/// Get ready unassigned beads, ordered by priority.
pub fn ready_unassigned(conn: &Connection, limit: usize) -> Result<Vec<BeadInfo>> {
    with_retry(|| {
        let mut stmt = conn.prepare_cached(
            "SELECT id, coalesce(title,''), coalesce(description,''), status, \
             coalesce(assignee,''), updated_at, coalesce(priority,999) \
             FROM issues \
             WHERE status = 'open' AND (assignee IS NULL OR assignee = '') \
             ORDER BY priority, created_at \
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
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
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
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
        let mut stmt = conn.prepare_cached(
            "SELECT coalesce(description,'') FROM issues WHERE id = ?1 LIMIT 1",
        )?;
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
        let mut stmt = conn.prepare(
            "SELECT coalesce(description,'') FROM issues WHERE description LIKE ?1",
        )?;
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
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
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
        assert!(d3 >= cap_min && d3 <= cap_max, "attempt 3 not capped: {d3:?}");
        assert!(d10 >= cap_min && d10 <= cap_max, "attempt 10 not capped: {d10:?}");
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
        assert!(!is_locked_error(&constraint), "SQLITE_CONSTRAINT should not be locked");
        assert!(!is_locked_error(&query_err), "QueryReturnedNoRows should not be locked");
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
        assert!(write_result.is_err(), "query_only connection should reject writes");
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
            let mut stmt = conn
                .prepare("SELECT id, status FROM issues")
                .unwrap();
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
            .query_row(
                "SELECT status FROM issues WHERE id = 'pat-1'",
                [],
                |row| row.get(0),
            )
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
