#!/usr/bin/env python3
"""Falsify the central claim of 2026-10-30-reverified-against-prebuilt-binaries.md.

That report tells workers that a specific set of rows in the 08:11 test-debt
snapshot are NOT work. If that is wrong, someone skips a real failure — so it is
the claim worth a replayable check.

What this asserts, for every row the report lists under "Confirmed stale rows":

  1. the row's target has a prebuilt binary dated TODAY (an older binary's
     "pass" describes older code and proves nothing about HEAD);
  2. running that binary filtered to the row reports 1 test, 0 failed;
  3. the row really is present in the snapshot it claims to correct.

Exits 0 only if every row passes all three. Any failure prints the row and
exits 1, so this is usable as a verification command.

No cargo: it runs binaries that already exist.
"""
from __future__ import annotations

import os
import re
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
REPORT = os.path.join(REPO, "docs/test-debt/2026-10-30-reverified-against-prebuilt-binaries.md")
SNAPSHOT = os.path.join(REPO, "docs/test-debt/2026-10-30-failing-tests-at-HEAD.txt")
DEPS = os.path.join(REPO, "engine-rs/target-verifier/debug/deps")
TODAY = "2026-10-30"
TIMEOUT = 60

# Rows are listed in the report as:  | `target::test::path` | HH:MM:SS |
ROW = re.compile(r"^\|\s*`(?P<target>[^:`|]+)::(?P<test>[^`|]+)`\s*\|\s*(?P<mtime>[\d:]+)\s*\|")
RESULT = re.compile(r"test result: \w+\. (?P<pass>\d+) passed; (?P<fail>\d+) failed")
BIN = re.compile(r"-[0-9a-f]{16}$")


def claimed_stale_rows() -> list[tuple[str, str]]:
    """The rows under the 'Confirmed stale rows' heading."""
    rows, inside = [], False
    for line in open(REPORT):
        if line.startswith("### Confirmed stale rows"):
            inside = True
            continue
        if inside and line.startswith(("##", "### ")):
            break
        if inside:
            m = ROW.match(line.strip())
            if m:
                rows.append((m.group("target"), m.group("test")))
    return rows


def newest_binary(target: str) -> str | None:
    cands = [
        os.path.join(DEPS, n)
        for n in os.listdir(DEPS)
        if n.startswith(target + "-")
        and BIN.search(n)
        and "." not in n
        and os.access(os.path.join(DEPS, n), os.X_OK)
    ]
    return max(cands, key=os.path.getmtime) if cands else None


def main() -> int:
    rows = claimed_stale_rows()
    if not rows:
        print("FAIL: parsed 0 rows from the report's 'Confirmed stale rows' table")
        return 1

    snapshot = {l.strip() for l in open(SNAPSHOT) if l.strip()}
    failures: list[str] = []

    for target, test in rows:
        full = f"{target}::{test}"

        if full not in snapshot:
            failures.append(f"{full}: not in the snapshot, so there is nothing to correct")
            continue

        binary = newest_binary(target)
        if binary is None:
            failures.append(f"{full}: no prebuilt binary for target {target}")
            continue

        mtime = time.strftime("%Y-%m-%d", time.localtime(os.path.getmtime(binary)))
        if mtime != TODAY:
            failures.append(
                f"{full}: binary is {mtime}, not {TODAY} — an old binary's pass "
                f"says nothing about HEAD"
            )
            continue

        try:
            proc = subprocess.run(
                [binary, test, "--test-threads=1"],
                capture_output=True,
                text=True,
                timeout=TIMEOUT,
                cwd=os.path.join(REPO, "engine-rs"),
            )
        except subprocess.TimeoutExpired:
            failures.append(f"{full}: timed out after {TIMEOUT}s")
            continue

        out = proc.stdout + proc.stderr
        m = RESULT.search(out)
        if not m:
            failures.append(f"{full}: could not parse a test result line")
        elif "ignored," in out:
            failures.append(
                f"{full}: listed as stale but it is #[ignore]d — that is parked "
                f"debt, not a fixed test"
            )
        elif int(m.group("pass")) != 1 or int(m.group("fail")) != 0:
            failures.append(
                f"{full}: expected 1 passed/0 failed, got "
                f"{m.group('pass')} passed/{m.group('fail')} failed"
            )
        else:
            print(f"ok   {full}  (binary {os.path.basename(binary)}, {mtime})")

    print()
    if failures:
        print(f"FAILED: {len(failures)} of {len(rows)} claimed-stale rows do not hold")
        for f in failures:
            print(f"  - {f}")
        return 1
    print(f"PASSED: all {len(rows)} rows the report calls stale pass in a binary built {TODAY}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
