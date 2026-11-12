# Patina Runtime Release Train

This document defines the repeatable release-train workflow for Patina runtime
milestones. The release train is a fixed-cadence process that converts the
work in `apps/orchestrator/`, `engine-rs/`, and the GDExtension compatibility
lab into shippable runtime milestones with measurable acceptance.

The goal is a predictable cadence — every milestone has explicit entry
criteria, exit criteria, an owned regression suite, and a worked example so
that contributors and the orchestrator both know what "done" looks like.

# Entry Criteria

A milestone enters the release train when **all** of the following hold:

- The milestone scope is captured as a parent bead in the `br` tracker with
  a `phase*` or `milestone:*` label, and the parent bead has at least one
  acceptance-criteria line authored via `br update --acceptance-criteria`.
- Every directly-blocking dependency bead is in `status=done` (verified by
  the verifier lane), or has been explicitly waived in the parent bead's
  comments with a written justification.
- A baseline parity / regression snapshot has been captured for the runtime
  surfaces the milestone touches (`engine-rs/tests/*_parity_test.rs` and
  `engine-rs/tests/*_trace_comparison_test.rs` are the canonical sources).
- The contributor onboarding doc (`docs/CONTRIBUTOR_ONBOARDING.md`) and
  this release-train doc both compile under their guard tests
  (`engine-rs/tests/docs_contributor_onboarding_test.rs`,
  `engine-rs/tests/docs_release_train_test.rs`).

# Exit Criteria

A milestone exits the release train and is tagged shippable when **all** of
the following hold:

- All beads under the milestone label are `status=done` and the verifier
  lane reports a clean `./scripts/rust_task.sh nextest run -p patina-engine`
  for the regression suite scoped to the milestone.
- The known-risk backlog for the milestone is owned: each open bug or
  parity gap must have an assignee and either a target follow-up milestone
  or an explicit "won't fix in v1" annotation in the bead body.
- Acceptance gates listed in `prd/PHASE9_HARDENING_AUDIT.md` (or the
  milestone's own audit doc) are recorded as **measured** with a pointer
  to the test file or doc that backs the claim — not just "implemented".
- The migration guide (`docs/migration-guide.md`) and crash-triage docs
  reflect any new public API or runtime regression-handling changes that
  the milestone introduced. Stale guidance must be removed, not left to
  rot.

# Cadence

The release train runs on a **two-week cadence** by default. The cadence
is not enforced by tooling — it is a coordination contract:

- **Week 1**: planning + scoping. The planner skill (`/loop /planner`)
  refreshes the bead queue, splits any planner-meta beads into concrete
  implementation beads (every bead must satisfy
  `feedback_beads_must_be_implementation`), and ensures every bead has
  real acceptance criteria.
- **Week 2 (early)**: the verifier lane runs the targeted regression
  suite for the milestone after each batch of completed beads, gating
  merges via `rust_task.sh`.
- **Week 2 (late)**: exit-criteria review. If any criterion is unmet,
  the milestone slips by one cadence rather than shipping with a
  silently-broken gate. Slips are recorded as a comment on the parent
  bead so future planners can see the slip history.

Out-of-band hotfix releases are permitted for runtime regressions that
break parity with Godot oracle traces; they reuse the same exit criteria
but skip the planning week.

# Example Milestone

A worked example for the **Phase 9 Hardening** milestone:

- Parent bead: `pat-d59t7` ("Define repeatable release-train workflow
  for Patina runtime milestones") with label `phase9`.
- Entry: all Phase 8 parity gates green
  (`engine-rs/tests/physics3d_trace_comparison_test.rs`,
  `engine-rs/tests/render_3d_parity_test.rs`), plus the audit doc
  `prd/PHASE9_HARDENING_AUDIT.md` enumerating each deliverable's
  evidence column (release train, contributor onboarding, migration
  guide, crash triage, fuzz/property, benchmark dashboards).
- Regression suite scoped to the milestone:
  `release_train_workflow_test.rs`,
  `docs_release_train_test.rs`,
  `docs_contributor_onboarding_test.rs`,
  `migration_guide_validation_test.rs`,
  `crash_triage_process_test.rs`.
- Exit: every row in the Phase 9 audit table is marked **Measured**
  with a concrete test or doc backing the claim, the known-risk backlog
  has owners, and the verifier lane reports a clean run via
  `./scripts/rust_task.sh nextest run -p patina-engine`.
- Cadence slip example: if the migration-guide breadth gap
  (`pat-1b7i6`) is not closed by exit-criteria review, the milestone
  slips one cadence rather than shipping with the gap silently
  unmeasured.
