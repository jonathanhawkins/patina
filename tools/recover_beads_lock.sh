#!/usr/bin/env bash
# Recover the beads DB from a stuck-lock / WAL-recovery deadlock.
#
# Symptom: every flywheel pane (coordinator/verifier/planner/worker) logs
#   "load failed: cannot connect to database: database is locked (261)"
#   path: .beads/beads.db
#
# 261 = SQLITE_BUSY_RECOVERY: a br writer died mid-flush leaving an
# un-checkpointed WAL that needs recovery, but long-lived read-only `bv`
# viewer TUIs hold read locks that block recovery. It is NOT corruption.
#
# Fix: release the read locks (kill stale bv viewers), checkpoint the WAL,
# verify integrity. Safe to run; bv viewers are read-only and can be relaunched.
set -euo pipefail

cd "$(dirname "$0")/.."
DB=".beads/beads.db"

echo "== holders of $DB =="
lsof "$DB" "$DB-wal" 2>/dev/null || true

# Kill stale bv viewers (read-only; they block WAL recovery). Leave writers alone.
BV_PIDS=$(lsof -t "$DB" 2>/dev/null | while read -r pid; do
  [ "$(ps -o comm= -p "$pid" 2>/dev/null)" = "bv" ] && echo "$pid"
done || true)
if [ -n "${BV_PIDS:-}" ]; then
  echo "== killing stale bv viewers: $BV_PIDS =="
  kill $BV_PIDS 2>/dev/null || true
  sleep 1
fi

echo "== checkpoint (TRUNCATE) =="
sqlite3 "$DB" 'PRAGMA wal_checkpoint(TRUNCATE);'

echo "== integrity check =="
RESULT=$(sqlite3 -readonly "$DB" 'PRAGMA integrity_check;')
echo "$RESULT"
if [ "$RESULT" != "ok" ]; then
  echo "== leaked pages / minor issues found; running VACUUM =="
  sqlite3 "$DB" 'VACUUM;'
  sqlite3 -readonly "$DB" 'PRAGMA integrity_check;'
fi

# Clear any orphan flush temp files from the crashed writer.
rm -f .beads/issues.jsonl.*.tmp 2>/dev/null || true

echo "== verify br =="
br ready --limit 1 >/dev/null && echo "br OK — flywheel can reconnect"
