# Patina Bead Execution Map

This file is the working guide for agents executing the post-V1 Patina backlog.

Use it as the operational layer on top of:

- [AGENTS.md](/Users/bone/dev/games/patina/AGENTS.md)
- [prd/PORT_GODOT_TO_RUST_PLAN.md](/Users/bone/dev/games/patina/prd/PORT_GODOT_TO_RUST_PLAN.md)
- [prd/V1_EXIT_CRITERIA.md](/Users/bone/dev/games/patina/prd/V1_EXIT_CRITERIA.md)
- [prd/PHASE6_3D_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE6_3D_PARITY_AUDIT.md)
- [prd/PHASE7_PLATFORM_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE7_PLATFORM_PARITY_AUDIT.md)
- [prd/PHASE8_EDITOR_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE8_EDITOR_PARITY_AUDIT.md)
- [prd/PHASE9_HARDENING_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE9_HARDENING_AUDIT.md)

## Current State

V1 runtime exit is complete.

- The measured V1 gate is green in [prd/V1_EXIT_CRITERIA.md](/Users/bone/dev/games/patina/prd/V1_EXIT_CRITERIA.md).
- Current execution work lives in audited post-V1 lanes, not the old V1 closure queue.
- Agents should prefer audit-backed beads that tighten measured scope, evidence, and documentation over broad roadmap work.

## Operating Rules

1. One main agent owns `br`.
2. Worker agents should implement or investigate code, but should not mutate bead state unless explicitly assigned to do so.
3. Always prefer the audited critical path over breadth.
4. Do not create or claim broad work that bypasses the phase audits.
5. Do not treat examples as proof of completion. Examples must feed fixtures, goldens, oracle coverage, or audited documentation.
6. When in doubt, choose the bead that creates measurable parity evidence or reconciles public claims with audited reality.
7. Do not duplicate beads when an audit-backed bead already exists for the same gap.

## Agent Execution Protocol

Use this protocol every time. Do not improvise around it.

1. The main agent is the only agent that should claim, update, or close beads in `br`.
2. Every worker must be assigned exactly one bead ID before starting work.
3. Every worker report must begin with the bead ID it is working on.
4. If a worker cannot name the bead it is implementing, it should stop and ask for assignment.
5. Do not start a new bead because code "looks related". Work must map to an existing bead or to an explicit audit finding approved by the main agent.
6. Do not open new parallel work in the same write area unless the main agent explicitly approves it.
7. If a worker discovers missing scope, it should propose a new bead instead of silently expanding the current one.
8. A bead may be closed only after tests, fixtures, or audited documentation verify its acceptance criteria.
9. Code changes without bead-state updates are considered workflow drift and should be corrected immediately.
10. If `br` is flaky, the main agent should still keep the bead ID in the worker prompt and report, then reconcile `br` state after the work lands.

### Required Worker Report Format

Every worker report should use this structure:

- `Bead`: `<ID> <TITLE>`
- `Changed files`: `<paths>`
- `Tests run`: `<commands>`
- `Status`: `done` or `blocked`
- `Risks`: `<remaining issues or none>`

### Required Main-Agent Loop

The main agent should repeat this loop:

1. Pick the next bead from `Now`, then `Next`
2. Mark or record that bead as active
3. Assign one worker per bead
4. Reject work that is not tied to a bead ID
5. Verify results against the bead acceptance criteria
6. Close the bead only after verification

## Claim Order

Agents should claim beads in this order:

1. `Now`
2. `Next`
3. `Later`
4. `Do Not Touch Yet` only if explicitly directed

## Now

These are the active post-V1 execution lanes. Do not bypass them with new broad phase work.

### Lane A: Phase 6 3D Runtime Parity

Source of truth:

- [prd/PHASE6_3D_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE6_3D_PARITY_AUDIT.md)

Claim in this order:

1. `pat-hx666` Phase 6 audit: classify the supported 3D runtime slice and crate boundaries
2. `pat-zaafu` Phase 6 parity: define the 3D fixture corpus from the audited class families
3. `pat-on9xe` Phase 6 parity: add comparison tooling for the audited 3D report dimensions
4. `pat-57aw6` Phase 6 parity: publish a 3D report that matches the audited support claims

### Lane B: Phase 7 Platform and Packaging Parity

Source of truth:

- [prd/PHASE7_PLATFORM_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE7_PLATFORM_PARITY_AUDIT.md)

Claim in this order:

1. `pat-2uc5z` Phase 7 audit: define supported desktop targets from the audited platform slice
2. `pat-vjmfv` Phase 7 parity: scope startup and packaging flow to the audited runtime artifact path
3. `pat-yzdv7` Phase 7 parity: keep `gdplatform` stable-layer claims aligned with the audit
4. `pat-s3700` Phase 7 parity: keep target-validation coverage aligned with the audited support matrix

## Next

These lanes should proceed once the active Phase 6-7 work is materially unblocked.

### Lane C: Phase 8 Editor-Facing Compatibility

Source of truth:

- [prd/PHASE8_EDITOR_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE8_EDITOR_PARITY_AUDIT.md)

Claim in this order:

1. `pat-6m9ky` Phase 8 parity: define the minimal editor-facing compatibility layer from the audited shell
2. `pat-4vy88` Phase 8 parity: enumerate tooling milestones from the audited editor slices
3. `pat-0rqx9` Phase 8 docs: keep the editor architecture plan aligned with the audited compatibility scope

### Lane D: Phase 9 Hardening and Release Readiness

Source of truth:

- [prd/PHASE9_HARDENING_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE9_HARDENING_AUDIT.md)

Claim in this order:

1. `pat-1b7i6` Phase 9 docs: keep the migration guide aligned with the audited runtime scope
2. `pat-ozyiw` Phase 9 docs: keep contributor onboarding aligned with validated runtime and oracle workflows
3. `pat-d59t7` Phase 9 docs: align the release-train workflow with committed automation and gates
4. `pat-t8hgz` Phase 9 hardening: keep crash triage docs aligned with the validated process model
5. `pat-3pstd` Phase 9 hardening: keep fuzz and property coverage focused on audited high-risk surfaces
6. `pat-5jwj9` Phase 9 hardening: keep benchmark dashboards aligned with committed baselines and gates

## Later

This work is valid, but it should be opened only after the current audited lanes produce concrete evidence or claim updates.

1. New audit-backed beads that decompose a documented gap in one of the phase 6-9 audit files
2. Additional fixture or oracle expansion that tightens a measured claim already recorded in the audits
3. Narrow documentation cleanup that reconciles public status pages with the audited scope

## Do Not Touch Yet

These are intentionally blocked unless explicitly directed by the project lead.

1. New broad subsystem initiatives that are not yet classified in the phase 6-9 audits
2. Duplicate beads for gaps already covered by an active or recently completed audit-backed bead
3. Public parity claims that exceed what the current tests, fixtures, or audit artifacts support
4. New editor feature expansion outside the audited maintenance and compatibility scope

## Parallel Work Rules

Use multiple agents only when the write scopes are cleanly separated.

Safe parallel combinations:

- one Phase 6 bead and one Phase 7 bead
- one Phase 6 or Phase 7 bead and one Phase 9 docs bead
- one Phase 8 docs bead and one Phase 9 hardening bead

Avoid parallel work when beads are likely to overlap in the same files:

- `pat-hx666`, `pat-zaafu`, `pat-on9xe`, `pat-57aw6`
- `pat-2uc5z`, `pat-vjmfv`, `pat-yzdv7`, `pat-s3700`
- `pat-6m9ky`, `pat-4vy88`, `pat-0rqx9`
- `pat-1b7i6`, `pat-ozyiw`, `pat-d59t7`

## Main-Agent Checklist

The main agent should do this every session:

1. Re-read [AGENTS.md](/Users/bone/dev/games/patina/AGENTS.md)
2. Open this file
3. Check `br list`
4. Check the relevant phase audit before claiming work
5. Claim from `Now`, then `Next`
6. Only close a bead when its acceptance criteria are verified by tests, fixtures, or audited documentation
7. Reconcile stale or duplicate beads before opening new broad work

## Worker-Agent Prompt Template

Use this shape when assigning work:

```text
You are working bead <ID> <TITLE>.

Read first:
- AGENTS.md
- prd/BEAD_EXECUTION_MAP.md
- the phase audit referenced by this bead

Rules:
- Do not mutate `br`
- Do not change scope beyond the bead
- Run the narrowest verification that proves the acceptance criteria
- Report back using the required worker report format
```
