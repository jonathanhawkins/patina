#!/usr/bin/env python3
"""Re-measure docs/test-debt/2026-10-30-failing-tests-at-HEAD.txt against the
prebuilt verifier binaries.

The snapshot is from one workspace run at ~08:11 local and nothing regenerates
it; I already proved one of its rows (resource_suite's animation test) passes
now, after claiming a repair for it. Every worker decomposing pat-21oab is
reading this file as ground truth, so the useful artifact is a re-measurement.

Method: for each target, run the NEWEST target-verifier binary whole, with
default parallelism (the snapshot was made by `--workspace --no-fail-fast`, so
parallel is the comparable condition), and diff its real failure set against the
snapshot's rows for that target.

No cargo: these binaries already exist, so this costs no build slot.
"""
import json
import os
import re
import subprocess
import sys
import time
from collections import defaultdict

REPO = "/Users/bone/dev/games/patina"
DEBT = f"{REPO}/docs/test-debt/2026-10-30-failing-tests-at-HEAD.txt"
DEPS = f"{REPO}/engine-rs/target-verifier/debug/deps"
OUT = sys.argv[1] if len(sys.argv) > 1 else "/tmp/reverify.json"
PER_BINARY_TIMEOUT = 20

# --- snapshot rows, grouped by target ---------------------------------------
snapshot = defaultdict(set)
for line in open(DEBT):
    line = line.strip()
    if not line or "::" not in line:
        continue
    target, rest = line.split("::", 1)
    snapshot[target].add(rest)

# --- newest binary per target ------------------------------------------------
# Cargo names them "<target>-<16 hex>"; anything else with the same prefix is a
# different target whose name merely starts the same, so the hash shape matters.
bin_re = re.compile(r"^(?P<t>.+)-[0-9a-f]{16}$")
candidates = defaultdict(list)
for name in os.listdir(DEPS):
    p = os.path.join(DEPS, name)
    if "." in name or not os.path.isfile(p) or not os.access(p, os.X_OK):
        continue
    m = bin_re.match(name)
    if m:
        candidates[m.group("t")].append(p)

FAIL_LINE = re.compile(r"^test (?P<name>\S+) \.\.\. FAILED", re.M)
RESULT = re.compile(
    r"test result: \w+\. (?P<pass>\d+) passed; (?P<fail>\d+) failed; "
    r"(?P<ignored>\d+) ignored"
)

results = {}
targets = sorted(snapshot)
for i, target in enumerate(targets, 1):
    paths = candidates.get(target)
    if not paths:
        results[target] = {"status": "no_binary", "snapshot_rows": len(snapshot[target])}
        print(f"[{i}/{len(targets)}] {target}: NO BINARY", flush=True)
        continue
    newest = max(paths, key=os.path.getmtime)
    mtime = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(os.path.getmtime(newest)))
    try:
        proc = subprocess.run(
            [newest],
            capture_output=True,
            text=True,
            timeout=PER_BINARY_TIMEOUT,
            cwd=f"{REPO}/engine-rs",
        )
        out = proc.stdout + proc.stderr
    except subprocess.TimeoutExpired:
        results[target] = {
            "status": "timeout",
            "binary": os.path.basename(newest),
            "binary_mtime": mtime,
            "snapshot_rows": len(snapshot[target]),
        }
        json.dump(results, open(OUT, "w"), indent=1)
        print(f"[{i}/{len(targets)}] {target}: TIMEOUT", flush=True)
        continue

    # The harness prints the roster under a `failures:` block; the per-test
    # "... FAILED" lines are the reliable machine-readable form.
    failed = {m.group("name") for m in FAIL_LINE.finditer(out)}
    tally = RESULT.search(out)
    snap = snapshot[target]
    entry = {
        "status": "ran",
        "binary": os.path.basename(newest),
        "binary_mtime": mtime,
        "tally": tally.groupdict() if tally else None,
        "snapshot_rows": len(snap),
        "actual_failures": len(failed),
        "still_failing": sorted(snap & failed),
        "now_passing": sorted(snap - failed),
        "newly_failing": sorted(failed - snap),
    }
    results[target] = entry
    json.dump(results, open(OUT, "w"), indent=1)
    flag = ""
    if entry["now_passing"]:
        flag += f"  STALE:{len(entry['now_passing'])}"
    if entry["newly_failing"]:
        flag += f"  NEW:{len(entry['newly_failing'])}"
    print(
        f"[{i}/{len(targets)}] {target}: snapshot={len(snap)} actual={len(failed)}{flag}",
        flush=True,
    )

json.dump(results, open(OUT, "w"), indent=1)

ran = [v for v in results.values() if v["status"] == "ran"]
print("\n==== SUMMARY ====")
print(f"targets in snapshot:      {len(targets)}")
print(f"  measured:               {len(ran)}")
print(f"  no binary:              {sum(1 for v in results.values() if v['status']=='no_binary')}")
print(f"  timed out:              {sum(1 for v in results.values() if v['status']=='timeout')}")
print(f"snapshot rows (measured targets): {sum(v['snapshot_rows'] for v in ran)}")
print(f"  still failing:          {sum(len(v['still_failing']) for v in ran)}")
print(f"  NOW PASSING (stale):    {sum(len(v['now_passing']) for v in ran)}")
print(f"newly failing (not in snapshot):  {sum(len(v['newly_failing']) for v in ran)}")
print(f"\nwrote {OUT}")
