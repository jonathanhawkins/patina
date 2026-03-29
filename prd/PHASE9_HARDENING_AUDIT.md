# Phase 9 Hardening and Release Discipline Audit

Date: 2026-03-29
Target upstream: Godot `4.6.1-stable`
Patina phase: `Phase 9 - Hardening and Release Discipline`

## Purpose

This document turns Phase 9 from a broad “release discipline” lane into a
concrete audit of the repo's current hardening and process artifacts.

It answers four questions:

1. Which Phase 9 deliverables already exist and are validated?
2. Which ones are measured as real workflow/tooling artifacts versus docs only?
3. Where do the docs overclaim relative to the checked-in artifacts?
4. Which remaining gaps should become beads without duplicating existing work?

## Audit Rules

Use this workflow for all future Phase 9 work.

1. Treat hardening/process deliverables as repo contracts, not aspirational roadmap text.
2. Keep “doc exists” separate from “doc is validated by tests”.
3. Classify each family as one of:
   - `Measured`
   - `Implemented, not yet measured`
   - `Deferred`
   - `Missing`
4. Do not create a new bead if an active or closed bead already covers the same validated outcome.
5. Prefer one bead per hardening/workflow cluster, not one bead per paragraph.

## Sources To Compare

### Phase 9 Plan Source

- `prd/PORT_GODOT_TO_RUST_PLAN.md` Phase 9 deliverables

### Local Docs and Tests

- `docs/BENCHMARK_BASELINES.md`
- `docs/RELEASE_PROCESS.md`
- `docs/contributor-onboarding.md`
- `docs/TRIAGE_PROCESS.md`
- `docs/migration-guide.md`
- `engine-rs/tests/benchmark_dashboard_test.rs`
- `engine-rs/tests/bench_runtime_baselines.rs`
- `engine-rs/tests/perf_benchmark_ci_gate_test.rs`
- `engine-rs/tests/release_train_workflow_test.rs`
- `engine-rs/tests/crash_triage_process_test.rs`
- `engine-rs/tests/crash_triage_auto_issue_test.rs`
- `engine-rs/tests/contributor_onboarding_docs_test.rs`
- `engine-rs/tests/contributor_onboarding_validation_test.rs`
- `engine-rs/tests/migration_guide_validation_test.rs`
- `engine-rs/tests/fuzz_property_coverage_test.rs`

## Current Patina Phase 9 Read

Phase 9 is not hypothetical.

The repo already contains:

- benchmark baselines and dashboard tooling
- benchmark regression-gate logic
- fuzz/property test modules across multiple subsystems
- crash triage models, queueing, and auto-issue generation
- contributor onboarding docs with validation tests
- migration guide validation tests
- release-train workflow contract tests

The main audit problem is not missing structure.

The real question is where the docs still imply:

- automation that is not yet definitely committed
- broader operational coverage than the validated artifacts prove

## Initial Phase 9 Classification

### First Matrix Rows

| Deliverable | Patina Area | Current Status | Evidence | Gap Type | Existing Bead | Action |
|-------------|-------------|----------------|----------|----------|---------------|--------|
| benchmark dashboards | `gdcore::dashboard`, benchmark docs/tests | Measured for local dashboard/tooling slice | `benchmark_dashboard_test.rs`, `perf_benchmark_ci_gate_test.rs`, `docs/BENCHMARK_BASELINES.md` | missing breadth | `pat-5jwj9` | reuse evidence, keep scoped to committed dashboard artifacts |
| fuzz/property tests where useful | fuzz/property modules across core/resource/script/variant/object-platform | Measured for current coverage gate | `fuzz_property_coverage_test.rs`, fuzz/property modules and tests | missing breadth | `pat-3pstd` | reuse evidence, keep scoped to high-risk surfaces |
| crash triage process | `gdcore::crash_triage`, docs/tests | Measured for local process/model slice | `crash_triage_process_test.rs`, `crash_triage_auto_issue_test.rs`, `docs/TRIAGE_PROCESS.md` | missing breadth | `pat-t8hgz` | reuse evidence |
| release train | release-process docs, workflow contract tests | Implemented, partly measured | `release_train_workflow_test.rs`, `docs/RELEASE_PROCESS.md` | docs-overclaim | `pat-d59t7` | narrow docs where automation is not yet guaranteed |
| contributor onboarding docs | onboarding docs/tests | Measured | `contributor_onboarding_docs_test.rs`, `contributor_onboarding_validation_test.rs`, `docs/contributor-onboarding.md` | none | likely active onboarding bead | reuse evidence |
| migration guide for users | migration guide/tests | Measured for doc structure and scope validation | `migration_guide_validation_test.rs`, `docs/migration-guide.md` | missing breadth | `pat-1b7i6` | reuse evidence, keep scoped to validated runtime guidance |

## Deliverable Notes

### Benchmark dashboards

- Current classification: `Measured for local dashboard/tooling slice`
- Reason:
  - The repo has dashboard models, regression detection, reporting, and CI-gate logic.
  - Benchmark baselines are documented and benchmark tests exist.
  - The strongest safe claim is a checked-in benchmark/dashboard framework, not necessarily a published external dashboard service.

### Fuzz/property test coverage

- Current classification: `Measured for current high-risk surface coverage`
- Reason:
  - The repo has explicit fuzz/property modules and a coverage gate across key subsystems.
  - This is enough to support the current “where useful” wording.
  - It should not be inflated into exhaustive fuzz coverage across the entire engine.

### Crash triage process

- Current classification: `Measured for local process/model slice`
- Reason:
  - The repo has severity classification, queueing, regression labeling, and auto-issue generation validated by focused tests.
  - That is a real process artifact, not just a prose guideline.

### Release train

- Current classification: `Implemented, partly measured`
- Reason:
  - Release-train contract tests cover workspace versioning, CI gates, test tiers, metadata, and regression-suite structure.
  - `docs/RELEASE_PROCESS.md` documents the workflow.
  - The main risk is doc overclaim: the doc currently states that `release.yml` automatically creates GitHub releases, but in this checkout the workflow is not yet a reliable committed artifact.

### Contributor onboarding

- Current classification: `Measured`
- Reason:
  - The onboarding guide has structure/validation tests and covers runtime/oracle/CI workflows.
  - This deliverable is already more concrete than the original broad planner wording.

### Migration guide

- Current classification: `Measured for validated user-guide structure`
- Reason:
  - The guide has validation tests and now reflects the bounded Phase 6-8 claims.
  - The remaining risk is keeping it aligned as future slices evolve.

## Existing Beads To Reuse

Do not create duplicates for these active Phase 9 beads:

- `pat-1b7i6` Draft migration guide for users adopting Patina runtime milestones
- `pat-d59t7` Define repeatable release-train workflow for Patina runtime milestones
- `pat-t8hgz` Define crash triage process for runtime regressions
- `pat-3pstd` Add fuzz and property tests for high-risk runtime surfaces
- `pat-5jwj9` Build benchmark dashboards for runtime parity and regressions

There are also already-live contributor-onboarding docs entries in the tracker.
Do not open another generic onboarding bead.

## Bead Candidates From This Audit

These are the first non-duplicative candidate tasks.

### Candidate 1

Title:
`Phase 9 docs: narrow release automation claims to the committed workflow surface`

Acceptance:

- `docs/RELEASE_PROCESS.md` describes release automation conditionally or cites the committed workflow artifact directly
- no Phase 9 doc implies automation that is not actually present in the repo

### Candidate 2

Title:
`Phase 9 audit: classify validated hardening artifacts versus docs-only process claims`

Acceptance:

- benchmark, fuzz/property, crash triage, release train, onboarding, and migration docs are labeled as measured, implemented-not-measured, deferred, or missing
- the audit cites the concrete tests or docs backing each claim

## Immediate Next Step

The next useful Phase 9 step is doc reconciliation, not more broad process beads:

- narrow release-process wording where automation is not yet guaranteed
- keep benchmark/onboarding/migration claims tied to their validated artifacts
- reuse the existing Phase 9 beads rather than opening duplicates
