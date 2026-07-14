---
name: dyld-unwedge
description: Diagnose and clear the macOS "_dyld_start launch wedge" (freshly built binaries hang at launch, verifier stalls, zero closes) WITHOUT a reboot. Trigger on: "dyld wedge", "_dyld_start", "binaries hang at launch", "AMFI wedge", "test binaries stuck 0% CPU", "unwedge", "HOST-ISSUE dyld".
---

# Clear the _dyld_start Launch Wedge (No Reboot)

Binaries hanging at `_dyld_start` with 0% CPU is **not** a kernel wedge that
requires a reboot. It is a **saturated syspolicyd/Gatekeeper launch-evaluation
queue**. Key mechanism (confirmed 2026-07-14 across two controlled episodes):

- Ad-hoc-signed (never-notarized) binaries — i.e. everything cargo builds — pay
  a serialized syspolicyd/XProtect evaluation **on launch, at a limited
  sustained rate** (not one-time per binary: a pre-warmed binary hangs again
  under storm load).
- `cargo nextest` spawns **one process per test**. A full-suite run = thousands
  of process spawns of huge binaries. At `NEXTEST_TEST_THREADS=8` this
  saturated the queue within a minute — twice. Every launch on the host then
  backs up indefinitely (even a fresh hello-world), which looks exactly like a
  permanent kernel wedge.
- Stop the storm and the queue drains in ~5–10 minutes. A quiet first launch of
  a big engine test binary takes ~5–25s; already-evaluated relaunches at low
  rate are instant.

## Step 1 — Confirm the signature (5s, always do this first)

90%+ of "wedge" reports are actually build-lock contention or a slow cold
build. Two checks:

```bash
# a) A hung binary's stack must bottom out at _dyld_start:
sample <hung-pid> 1 | grep -c _dyld_start   # >0 with 0:00.00 CPU = signature

# b) The decisive probe — a fresh binary that touches NO cargo/lock state:
printf 'int main(){return 0;}' >/tmp/probe.c && /usr/bin/clang /tmp/probe.c -o /tmp/probe_fresh
(/tmp/probe_fresh & P=$!; sleep 5; kill -0 $P 2>/dev/null && { echo QUEUE_SATURATED; kill -9 $P; } || echo NOT_WEDGED)
```

`NOT_WEDGED` → it's contention or a slow build (see
`project_flywheel_cold_build_timeout` memory). Stop here; do not "unwedge".

## Step 2 — Stop the launch storm

Every additional launch deepens the backlog. Kill everything that spawns new
binaries — but scope the deps kill to the ENGINE target dir (a bare
`target/debug/deps/` pattern collateral-kills other crates' test runs):

```bash
pkill -9 -f "cargo-nextest"; pkill -9 -f "nextest run"
pkill -f "rust_task"; pkill -f "am-run.*rust-build"
pkill -9 -f "engine-rs/target/debug/deps/"
rm -f .orchestrator/rust-build.lock.info; rmdir .orchestrator/rust-build.lock 2>/dev/null
```

If the flywheel is running, `/flywheel-stop` does this properly.

## Step 3 — Drain quietly (5–10 minutes)

Launch NOTHING new. Then re-run the Step 1b probe; it should complete in well
under a second. If it still hangs after ~10 quiet minutes across two probes,
only then is a reboot on the table (never yet needed since this diagnosis).

## Step 4 — Resume with a launch pattern the queue can sustain

The fix is **launch rate**, not pre-warming (pre-warm alone was disproven —
the storm re-wedged pre-warmed binaries):

- **Bulk validation runs**: use `cargo test` (one process per test *binary*,
  tests threaded in-process — ~64 spawns instead of ~8,300):
  `cargo test --tests -p patina-engine -p gdeditor --no-fail-fast`
- **nextest**: keep `NEXTEST_TEST_THREADS=1` (the rust_task.sh default — it
  exists for exactly this reason; do NOT override it upward for big runs).
- **Flywheel**: normal operation (scoped `--test` verifies, -j1) is fine.

## Permanent host-level fix worth enabling

System Settings → Privacy & Security → **Developer Tools** → add the terminal
(Ghostty/Terminal/iTerm). Processes spawned by a Developer-Tools-exempted app
skip Gatekeeper launch evaluation entirely — this removes the saturation
mechanism for local dev builds. Re-verify after enabling by re-running a
high-rate nextest fan-out.

## Related

- Memory: `project_amfi_codesign_launch_wedge` (full history + corrections)
- `/flywheel-stop` — proper storm shutdown; `/flywheel-start` — relaunch
- Orchestrator breaker: `verifier.rs` `circuit_breaker` (HOST-ISSUE alert text
  points here)
