# Patina Bead Execution Map

This file is the working guide for agents executing the Patina port backlog.

Use it as the operational layer on top of:

- [AGENTS.md](/Users/bone/dev/games/patina/AGENTS.md)
- [prd/PORT_GODOT_TO_RUST_PLAN.md](/Users/bone/dev/games/patina/prd/PORT_GODOT_TO_RUST_PLAN.md)
- [prd/BEAD_BACKLOG.md](/Users/bone/dev/games/patina/prd/BEAD_BACKLOG.md)

## Operating Rules

1. One main agent owns `br`.
2. Worker agents should implement or investigate code, but should not mutate bead state unless explicitly assigned to do so.
3. Always prefer the critical path over breadth.
4. Do not work on editor expansion while runtime parity exits remain open.
5. Do not treat examples as proof of completion. Examples must feed fixtures, goldens, or oracle coverage.
6. When in doubt, choose the bead that creates measurable oracle, render, physics, or runtime evidence.

## Agent Execution Protocol

Use this protocol every time. Do not improvise around it.

1. The main agent is the only agent that should claim, update, or close beads in `br`.
2. Every worker must be assigned exactly one bead ID before starting work.
3. Every worker report must begin with the bead ID it is working on.
4. If a worker cannot name the bead it is implementing, it should stop and ask for assignment.
5. Do not start a new bead because code \"looks related\". Work must map to an existing bead.
6. Do not open new parallel work in the same write area unless the main agent explicitly approves it.
7. If a worker discovers missing scope, it should propose a new bead instead of silently expanding the current one.
8. A bead may be closed only after tests, fixtures, or oracle evidence verify its acceptance criteria.
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

These beads are the active critical path. Do not bypass them.

1. `pat-i5c` Make frame processing semantics match Godot contracts
2. `pat-gnt` Generate upstream frame-trace golden for `test_scripts`
3. `pat-9j5` Compare Patina frame traces against upstream frame-trace goldens

Exit condition for this phase:

- Runtime frame sequencing is stable.
- Upstream `frame_trace` artifacts exist.
- Patina frame traces compare directly against upstream frame traces.

## Next

These beads should begin as soon as `pat-i5c` is materially unblocked or complete.

### Lane A: Runtime Traces and Signals

Claim in this order:

1. `pat-b16` Add global lifecycle and signal ordering trace parity
2. `pat-x8u` Finish scene-aware signal dispatch parity
3. `pat-fbi` Compare lifecycle notification traces against oracle output
4. `pat-fu6` Compare runtime signal traces against oracle trace output
5. `pat-isl` Expand notification coverage beyond lifecycle basics

### Lane B: Physics Integration

Claim in this order:

1. `pat-wbd` Connect `gdphysics2d` to scene nodes and fixed-step runtime
2. `pat-rhe` Sync Node2D body nodes with `gdphysics2d` world state
3. `pat-kxa` Advance `gdphysics2d` from `MainLoop` fixed-step frames
4. `pat-cyf` Add collision shape registration and overlap coverage
5. `pat-clv` Add `CharacterBody2D` and `StaticBody2D` behavior fixtures
6. `pat-1za` Add deterministic physics trace goldens
7. `pat-yxp` Add `physics_playground` golden trace fixture

### Lane C: Input and Platform Runtime

Claim in this order:

1. `pat-isw` Expose engine-owned input snapshot and routing API
2. `pat-g9k` Cover keyboard action snapshots through engine input API
3. `pat-vih` Add input-map loading and action binding coverage
4. `pat-aro` Add mouse position and button routing to input snapshots

### Lane D: Resources and Object Model

Claim in this order:

1. `pat-law` Integrate resource UID and cache behavior into loader paths
2. `pat-riz` Resolve `res://` and UID lookups through one loader path
3. `pat-2iu` Add repeated-load cache deduplication regression tests
4. `pat-cde` Close object/property reflection gaps in `gdobject`
5. `pat-rsq` Handle ext-resource and subresource edge cases in `PackedScene` loading
6. `pat-ooe` Validate `PackedScene` instancing ownership and unique-name behavior
7. `pat-h6a` Implement measurable `ClassDB` parity for core runtime classes

### Lane E: Godot Lab / Oracle Inputs

Claim in this order:

1. `pat-1wt` Expand `apps/godot` probes for API and resource validation
2. `pat-9eb` Probe `ClassDB` and node API signatures from `apps/godot`
3. `pat-a41` Probe resource metadata and roundtrip behavior from `apps/godot`
4. `pat-9k9` Automate API extraction from pinned upstream Godot

## Later

These beads should start only after the `Now` phase is closed and at least part of `Next` is green.

### Lane F: Render and 2D Slice Measurement

Claim in this order:

1. `pat-pd6` Measure one end-to-end 2D vertical slice from fixtures
2. `pat-wb3` Validate 2D draw ordering, visibility, and layer semantics
3. `pat-sfn` Extend camera and viewport render parity coverage
4. `pat-22g` Cover texture draw and sprite property parity in renderer fixtures
5. `pat-ijc` Add CI execution path for render golden tests
6. `pat-6t3` Add render benchmark fixtures and reporting

### Lane G: Example Cleanup

Claim in this order:

1. `pat-4y7` Remove example-local loop orchestration from platformer and shooter demos
2. `pat-icz` Break `winit` backend responsibilities into runtime-owned services
3. `pat-oa3` Normalize display and window state flow through `gdplatform`
4. `pat-v1w` Add window lifecycle and resize flow coverage
5. `pat-4qa` Map each example to a measurable fixture target
6. `pat-sgp` Map platformer demo to physics and render fixture targets
7. `pat-3zp` Map space shooter demo to oracle and render fixture targets
8. `pat-bma` Classify editor example as maintenance-only workflow support
9. `pat-2st` Document engine input contract for examples and tests

### Lane H: CI, Performance, and Reporting

Claim in this order:

1. `pat-gkv` Add CI checks for stale oracle and golden artifacts
2. `pat-91c` Define tiered test suites for fast runtime versus heavy golden coverage
3. `pat-hvv` Add headless runtime benchmark baselines
4. `pat-qv4` Split compatibility docs into measured status versus deferred scope
5. `pat-ati` Break final v1 port exit criteria into measurable subsystem checklists
6. `pat-bq7` Record third-party implementation strategy before wider subsystem imports

## Do Not Touch Yet

These are intentionally deferred until runtime and measured 2D parity are substantially stable.

1. `pat-j2u` Gate new `gdeditor` feature work behind runtime parity exits
2. `pat-v9w` Add explicit runtime-parity gate to `AGENTS` and flywheel docs
3. `pat-s60` Label editor test coverage as maintenance-only until runtime exits
4. `pat-nzg` Separate editor server stability work from editor feature backlog
5. `pat-kzr` Add maintenance-only reliability checks for editor server smoke paths
6. `pat-bwg` Reclassify 3D-adjacent scaffolding out of 2D milestone reporting
7. `pat-dd3` Define minimal audio milestone and stub contract
8. `pat-kaa` Add smoke coverage for `AudioStreamPlayer` stub behavior

## Parallel Work Rules

Use multiple agents only when the write scopes are cleanly separated.

Safe parallel combinations:

- `pat-b16` and `pat-law`
- `pat-wbd` and `pat-1wt`
- `pat-isw` and `pat-law`
- `pat-g9k` and `pat-riz`
- `pat-9eb` and `pat-a41`
- `pat-wb3` and `pat-sfn`

Avoid parallel work when beads are likely to overlap in the same files:

- `pat-b16`, `pat-x8u`, `pat-fbi`, `pat-fu6`
- `pat-wbd`, `pat-rhe`, `pat-kxa`, `pat-cyf`, `pat-clv`
- `pat-law`, `pat-riz`, `pat-2iu`, `pat-cde`, `pat-rsq`, `pat-ooe`
- `pat-4y7`, `pat-icz`, `pat-oa3`

## Main-Agent Checklist

The main agent should do this every session:

1. Re-read [AGENTS.md](/Users/bone/dev/games/patina/AGENTS.md)
2. Open this file
3. Check `br list`
4. Check `br dep tree <current-bead>`
5. Claim from `Now`, then `Next`
6. Only close a bead when its acceptance criteria are verified by tests, fixtures, or oracle evidence

## Worker-Agent Prompt Template

Use this when delegating:

```text
You are working on bead <ID> <TITLE>.
Follow AGENTS.md and prd/BEAD_EXECUTION_MAP.md.
Do not mutate br state.
Do not touch files outside the bead's likely write scope unless necessary.
Add tests with the implementation.
Report: changed files, tests run, remaining risks.
```
