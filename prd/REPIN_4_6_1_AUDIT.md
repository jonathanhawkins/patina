# Godot 4.6.1 Repin — Patina Behavior Delta Audit

**Date:** 2026-03-20
**Author:** repin-agent
**Scope:** Patina-facing behavioral changes introduced by pinning upstream to Godot 4.6.1 (from 4.5.1).

---

## Summary

Patina's upstream submodule (`upstream/godot`) was repinned from Godot 4.5.1 to Godot 4.6.1. This document records the outcome of the post-repin behavioral audit.

**Verdict: No behavioral differences found for Patina's scope.**

---

## What Was Checked

| Area | Method | Result |
|------|--------|--------|
| Oracle outputs | Regenerated via `tools/oracle/run_all.sh` | No delta vs 4.5.x |
| Benchmark baselines | `cargo run --example benchmarks` | Within thresholds (see BENCHMARKS.md) |
| Editor REST API suite | `cargo test -p gdeditor` | All tests pass |
| GDExtension lab | Probe stubs reviewed | Compiles; runtime deferred (see below) |
| Scene/resource parsing | `cargo test --workspace` suite | All passing |
| Physics behavior | Golden tests | Unchanged |
| Render goldens | `make test-render` suite | Unchanged |

---

## Godot 4.6.1 Release Notes — Patina-Relevant Items

Godot 4.6.1 is a patch release (bug fixes and minor improvements). The areas
relevant to Patina's current scope (scene tree, resource loading, scripting
surface, physics, rendering, editor API) showed no behavioral changes in:

- `ClassDB` signatures for the 17 core classes Patina probes
- `.tscn` / `.tres` parse output for existing fixture files
- Signal semantics (connect/emit ordering, argument passing)
- Physics step output for fixture scenes
- Render pixel output for golden scenes

No API removals or incompatible changes were identified in the 4.6.x changelog
that affect Patina's implemented surface.

---

## Items Deferred

| Item | Reason | Tracking |
|------|--------|---------|
| GDExtension lab live run | Requires Godot 4.6.1 binary + godot-rust compilation | pat-o37e |

---

## Oracle Regeneration

Oracle outputs were regenerated after the repin. All golden files matched
the 4.5.x outputs — no changes committed to `fixtures/` or `tests/golden/`.

See `engine-rs/TESTING.md` for the oracle regeneration procedure.
