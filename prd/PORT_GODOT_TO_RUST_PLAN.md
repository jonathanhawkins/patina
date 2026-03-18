# Godot to Rust Port Plan

## Executive Summary

We are **not** doing a file-by-file translation of Godot's C++ codebase into Rust.

We are building a **staged, behavior-compatible Rust runtime** that uses upstream Godot as the reference implementation and Flywheel as the planning and execution framework.

The port will proceed in controlled layers:

1. Build the planning and swarm infrastructure.
2. Turn upstream Godot into a behavioral oracle.
3. Use `GDExtension` and `godot-rust` as the proving ground for compatibility assumptions.
4. Implement a headless Rust runtime first.
5. Ship a 2D vertical slice.
6. Expand to 3D, platform support, and later editor parity.

The core principle is:

> **Port behavior and contracts, not source files.**

---

## Goals

- Build a Rust runtime that can load and run a meaningful subset of existing Godot projects.
- Measure parity against upstream Godot with automated compatibility tests.
- Use Flywheel to manage planning, decomposition, task routing, and multi-agent execution.
- Keep the implementation modular, testable, and safe.
- Reach production-quality milestones in phases rather than attempting a one-shot rewrite.

## Non-Goals

- Full editor parity in v1.
- Immediate support for every Godot subsystem, module, and platform.
- Blindly porting everything in `thirdparty/` to Rust.
- Translating C++ files one-by-one without rethinking architecture.
- Treating visual similarity as proof of correctness.

---

## Project Definition

### Target Outcome for v1

A Rust runtime that can:

- load scenes and resources,
- run a subset of `SceneTree` behavior,
- support a core object model,
- execute deterministic compatibility fixtures,
- render a first meaningful 2D slice,
- and demonstrate measurable parity with upstream Godot.

### Target Outcome for Later Milestones

- broader 2D coverage,
- 3D runtime coverage,
- platform/window/input layers,
- editor-facing APIs,
- partial then broader editor parity.

---

## Core Strategy

### 1. Use Flywheel as the control plane

Flywheel should manage the project at the planning and execution layer:

- foundation documents,
- master plan generation,
- bead decomposition,
- bead refinement,
- dependency routing,
- multi-agent execution,
- post-run learning from prior sessions.

### 2. Treat upstream Godot as the oracle

Use upstream Godot to define expected behavior for:

- object lifecycle,
- signals and notifications,
- scene-tree semantics,
- resource loading and serialization,
- import/export behavior,
- rendering snapshots,
- physics traces,
- and error behavior where practical.

### 3. Use Rust inside Godot before replacing Godot

Use `GDExtension` and `godot-rust` early to:

- validate assumptions,
- build compatibility tools,
- generate fixtures,
- probe API surfaces,
- and learn the real invariants before deeper replacement.

### 4. Build bottom-up, but ship vertical slices

We will build foundational runtime layers first, but every major milestone must produce a usable end-to-end slice.

### 5. Port contracts, not implementation details

Where upstream behavior is observable and testable, that behavior becomes the contract. The Rust implementation is free to differ internally as long as it preserves required semantics.

### 6. Handle `thirdparty/` as a separate strategy problem

Each dependency gets one of four outcomes:

- replace with a Rust crate,
- wrap via FFI,
- vendor as-is,
- or reimplement cleanly only when justified.

---

## Flywheel Execution Model

### Core Components

Use the Flywheel stack as follows:

- **ACFS**: standardize machine setup and agent environment.
- **NTM**: spawn and coordinate coding agents.
- **Agent Mail**: identity, messaging, and advisory file reservations.
- **Beads Rust + bv**: dependency graph, critical path, and prioritized execution.
- **CASS**: mine prior sessions for repeated failures and useful patterns.
- **DCG**: block destructive shell and git operations.

### Working Rules

- Every agent must reread `AGENTS.md` after compaction or context resets.
- Every code change must map to a bead.
- Every bead must have explicit acceptance criteria.
- File reservations are required before edits to shared areas.
- No destructive cleanup without explicit human approval.
- Every merged change must add or improve tests unless the bead explicitly documents why not.

---

## Foundation Documents

Create these documents before significant implementation work:

- `AGENTS.md`
- `PORT_SCOPE.md`
- `ARCHITECTURE_MAP.md`
- `COMPAT_MATRIX.md`
- `RISK_REGISTER.md`
- `TEST_ORACLE.md`
- `BENCHMARKS.md`
- `THIRDPARTY_STRATEGY.md`
- `CRATE_BOUNDARIES.md`
- `MILESTONES.md`

### Purpose of Each Document

#### `AGENTS.md`

Defines:

- coding conventions,
- branching rules,
- file reservation policy,
- test requirements,
- logging/reporting format,
- unsafe Rust policy,
- no-destructive-commands rules,
- and how agents should recover after compaction.

#### `PORT_SCOPE.md`

Defines:

- what counts as success for v1,
- what is deferred,
- supported fixture classes,
- initial platform targets,
- and compatibility boundaries.

#### `ARCHITECTURE_MAP.md`

Maps upstream Godot subsystems into Rust crates and workstreams.

#### `COMPAT_MATRIX.md`

Tracks support status for:

- core runtime,
- resources,
- scene system,
- signals,
- 2D rendering,
- physics,
- audio,
- input,
- 3D,
- scripting interop,
- editor support,
- and platform targets.

#### `TEST_ORACLE.md`

Defines how upstream Godot becomes the source of truth for expected behavior.

#### `BENCHMARKS.md`

Defines:

- baseline workloads,
- performance metrics,
- memory metrics,
- determinism criteria,
- and acceptable regressions.

---

## Recommended Initial Repository Layout

```text
repo/
  AGENTS.md
  PORT_SCOPE.md
  ARCHITECTURE_MAP.md
  COMPAT_MATRIX.md
  RISK_REGISTER.md
  TEST_ORACLE.md
  BENCHMARKS.md
  THIRDPARTY_STRATEGY.md
  CRATE_BOUNDARIES.md
  MILESTONES.md

  upstream/
    godot/

  engine-rs/
    Cargo.toml
    crates/
      gdcore/
      gdvariant/
      gdobject/
      gdresource/
      gdscene/
      gdserver2d/
      gdrender2d/
      gdphysics2d/
      gdaudio/
      gdplatform/
      gdscript-interop/
      gdeditor/

  tools/
    oracle/
    api-extract/
    fixtures/
    render-diff/
    physics-diff/
    benchmarks/

  fixtures/
    scenes/
    projects/
    resources/
    imports/
    physics/
    render/

  tests/
    compat/
    integration/
    golden/
    perf/
```

---

## Proposed Crate Boundaries

### Core Crates

- `gdcore`: low-level engine primitives, IDs, alloc/model helpers, diagnostics.
- `gdvariant`: `Variant`, conversion rules, typed value containers, serialization helpers.
- `gdobject`: object model, inheritance metadata, signals, notifications, refcounting hooks.
- `gdresource`: resources, loaders, savers, cache, UID/path semantics.
- `gdscene`: `Node`, `SceneTree`, packed scenes, instancing, lifecycle.

### Runtime Service Crates

- `gdserver2d`: abstract 2D server-facing runtime surface.
- `gdrender2d`: 2D rendering implementation and render testing adapters.
- `gdphysics2d`: 2D physics implementation and deterministic test harness.
- `gdaudio`: audio runtime, stream plumbing, basic mixer abstractions.
- `gdplatform`: windowing, input, timing, OS integration.

### Higher-Level Crates

- `gdscript-interop`: compatibility layer for scripting/runtime interop decisions.
- `gdeditor`: later-phase editor-facing layers.

### Cross-Cutting Support

- `tools/oracle`: upstream behavior capture.
- `tools/api-extract`: contract extraction from API definitions.
- `tests/compat`: parity tests against upstream outputs.

---

## Technical Principles

### Memory Safety

- Prefer safe Rust by default.
- Isolate `unsafe` behind narrow audited interfaces.
- Require explicit rationale for every `unsafe` block.
- Keep ownership boundaries obvious at crate seams.

### Determinism

- Prefer deterministic fixtures for first-pass parity work.
- Snapshot outputs wherever practical.
- Make nondeterministic behavior explicit in test metadata.

### Compatibility Over Mimicry

- Match behavior that matters to projects.
- Do not preserve internal C++ structure unless it buys compatibility or maintainability.

### Performance Discipline

- Do not optimize blindly.
- Establish upstream baselines.
- Track performance continuously from the first executable slice.

### Legal/Clean-Room Discipline

- Preserve licensing metadata and provenance.
- Treat vendored third-party code as an explicit licensing and architecture workstream.
- Prefer behavior-driven specs and tests over direct source translation where feasible.

---

## Phase Plan

## Phase 0 - Foundation and Planning

### Objectives

- Build the planning system.
- Define scope.
- Establish repo structure.
- Produce the master plan.
- Convert the plan into beads.

### Deliverables

- all foundation documents created,
- upstream submodule pinned,
- Rust workspace bootstrapped,
- first master plan drafted,
- bead graph created,
- agent roles defined,
- DCG enabled,
- reservation policy live.

### Exit Criteria

- no major coding starts without corresponding beads,
- core documents reviewed and internally consistent,
- first 50 to 100 beads prioritized,
- first swarm run can proceed without ambiguity.

---

## Phase 1 - Oracle, Fixtures, and Contracts

### Objectives

- Turn upstream Godot into a measurable oracle.
- Build the fixture corpus.
- Extract and normalize API contracts.

### Deliverables

- fixture corpus for scenes, resources, rendering, and physics,
- upstream dumpers for scene tree, properties, signals, resource roundtrips,
- API extraction pipeline,
- golden-output format,
- compatibility dashboard.

### Exit Criteria

- upstream outputs can be generated automatically,
- fixture behavior is versioned and reproducible,
- contracts are available to implementation teams.

---

## Phase 2 - GDExtension Compatibility Lab

### Objectives

- Use Rust inside real Godot to validate assumptions before deeper runtime replacement.
- Build diagnostic and compatibility helpers.

### Deliverables

- `GDExtension` harness,
- `godot-rust` smoke-test modules,
- scene and resource inspectors,
- signal/notification tracing helpers,
- API coverage tooling.

### Exit Criteria

- Rust tools can run inside upstream Godot reliably,
- major contract misunderstandings are identified early,
- fixture generation and runtime inspection are stable.

---

## Phase 3 - Headless Rust Runtime

### Objectives

Build the first independent Rust runtime slice without rendering dependency.

### Scope

- core types,
- object model,
- signals,
- notifications,
- resource load/save subset,
- packed scene subset,
- `MainLoop` subset,
- `SceneTree` subset,
- deterministic execution of basic scenes.

### Deliverables

- `gdvariant`, `gdobject`, `gdresource`, and `gdscene` first working versions,
- headless runner,
- compat tests for simple scene execution,
- resource roundtrip tests,
- signal ordering tests.

### Exit Criteria

- simple fixtures load and execute in Rust,
- object lifecycle semantics are stable,
- parity tests pass for agreed fixture set.

---

## Phase 4 - 2D Vertical Slice

### Objectives

Deliver the first meaningful end-to-end graphical milestone.

### Scope

- 2D node subset,
- transforms,
- sprite/basic draw path,
- input subset,
- timing/frame loop,
- first 2D physics integration as needed for fixtures.

### Deliverables

- `gdserver2d`, `gdrender2d`, and `gdphysics2d` initial implementations,
- render snapshot tests,
- simple 2D demo project compatibility,
- baseline performance measurements.

### Exit Criteria

- at least one real 2D project or representative fixture set runs,
- render outputs stay within agreed diff thresholds,
- frame loop correctness is testable and repeatable.

---

## Phase 5 - Broader Runtime and 3D Prep

### Objectives

Expand coverage after 2D slice is stable.

### Scope

- richer resource types,
- audio basics,
- broader input handling,
- scene instancing edge cases,
- groundwork for 3D servers and render paths.

### Deliverables

- improved compatibility matrix,
- broader integration fixtures,
- first audio test harness,
- initial 3D architecture spec.

### Exit Criteria

- core runtime no longer depends on milestone-specific hacks,
- broadened scene/resource support is measurable,
- 3D work can begin on a stable base.

---

## Phase 6 - 3D Runtime Slice

### Objectives

Deliver the first meaningful 3D milestone.

### Scope

- 3D node subset,
- transforms/cameras/lights subset,
- initial 3D render path,
- initial 3D physics hooks,
- representative 3D fixtures.

### Deliverables

- first 3D crate set,
- 3D fixture corpus,
- render and physics comparison tooling,
- first real 3D demo parity report.

### Exit Criteria

- representative 3D fixtures run,
- performance and correctness are measurable,
- platform/runtime boundaries remain clean.

---

## Phase 7 - Platform Layer and Distribution

### Objectives

Harden the runtime so it behaves like a real engine target.

### Scope

- windowing,
- input,
- timing,
- packaging/bootstrap,
- target platform abstractions,
- CI and artifact generation.

### Deliverables

- `gdplatform` first stable layer,
- desktop platform targets,
- startup/runtime packaging flow,
- CI matrix for supported targets.

### Exit Criteria

- runtime can be built and run in a repeatable way across initial target platforms,
- platform-specific code remains isolated.

---

## Phase 8 - Editor-Facing Work

### Objectives

Approach editor support only after runtime foundations are stable.

### Scope

- editor APIs,
- tooling hooks,
- inspectors,
- import pipeline surfaces,
- partial then broader editor features.

### Deliverables

- editor architecture plan,
- minimal editor-facing compatibility layer,
- selected tooling parity milestones.

### Exit Criteria

- editor work does not destabilize runtime milestones,
- runtime-first architecture remains intact.

---

## Phase 9 - Hardening and Release Discipline

### Objectives

Turn milestone success into sustained project health.

### Deliverables

- benchmark dashboards,
- fuzz/property tests where useful,
- crash triage process,
- release train,
- contributor onboarding docs,
- migration guide for users.

### Exit Criteria

- repeatable release cadence,
- stable regression suite,
- known-risk backlog clearly owned.

---

## Upstream Oracle Strategy

### What We Need to Capture from Upstream Godot

For every fixture class we should be able to produce machine-readable outputs such as:

- scene tree structure,
- node/property values,
- signal emission order,
- notifications and lifecycle events,
- resource serialization results,
- import pipeline outputs,
- render snapshots,
- physics traces,
- timing/frame progression summaries.

### Oracle Rules

- Upstream Godot is the reference for expected observable behavior.
- Rust implementation is allowed to differ internally.
- Every compatibility test must state what observable behavior it checks.
- When upstream behavior is ambiguous or version-sensitive, document it explicitly in `TEST_ORACLE.md`.

---

## Third-Party Dependency Strategy

Each dependency in upstream `thirdparty/` should be classified into one of these buckets:

1. **Replace with Rust crate**
2. **Wrap via FFI**
3. **Vendor unchanged**
4. **Reimplement cleanly**

### Decision Factors

- license burden,
- maintenance cost,
- performance sensitivity,
- API surface size,
- maturity of existing Rust ecosystem options,
- portability requirements,
- and debugging complexity.

### Rule

No team should start reimplementing third-party code until the classification decision has been made and recorded in `THIRDPARTY_STRATEGY.md`.

---

## Testing Strategy

### Test Types

- **Golden tests**: compare serialized or textual outputs to checked-in expectations.
- **Parity tests**: compare Rust outputs to upstream oracle outputs.
- **Integration tests**: run representative scene/project workflows.
- **Render diff tests**: compare image snapshots within defined thresholds.
- **Physics trace tests**: compare deterministic simulation traces.
- **Performance tests**: benchmark time, memory, startup, and frame behavior.

### Definition of Done for Any Runtime Bead

A bead is complete only when:

- implementation is merged,
- tests are added or updated,
- compatibility impact is recorded,
- docs are updated if the behavior surface changed,
- and any `unsafe` use is documented and justified.

---

## Bead Planning Structure

### Bead Pack 00 - Foundation

- create foundation docs,
- bootstrap repo layout,
- set up workspace,
- pin upstream submodule,
- enable DCG,
- define reservation rules,
- define reporting format,
- generate first prioritized bead graph.

### Bead Pack 01 - Oracle and Fixtures

- create minimal fixture corpus,
- write scene-tree dumper,
- write property dumper,
- write signal/notification tracer,
- build resource roundtrip tool,
- define golden formats,
- wire compatibility dashboard.

### Bead Pack 02 - API and Contract Extraction

- parse API definitions,
- normalize types,
- generate support matrix,
- produce crate-boundary contract docs,
- identify impossible/awkward surfaces early.

### Bead Pack 03 - Core Runtime

- implement `Variant` subset,
- implement `StringName`/IDs as needed,
- implement object registration and metadata,
- implement signals,
- implement notifications,
- implement refcount/lifetime model,
- implement core errors/logging.

### Bead Pack 04 - Resources and Scenes

- resource identifiers and paths,
- resource loader/saver subset,
- packed scene subset,
- node creation and parenting,
- enter/ready/process flow,
- simple scene execution tests.

### Bead Pack 05 - Headless Compatibility

- headless runner,
- deterministic fixture execution,
- upstream diff pipeline,
- CI integration for compat checks.

### Bead Pack 06 - 2D Runtime Slice

- transforms,
- sprite/basic draw path,
- frame loop,
- input subset,
- first render snapshot testing,
- first demo project run.

### Bead Pack 07 - Physics and Audio Expansion

- deterministic 2D physics subset,
- audio primitives,
- richer fixtures,
- baseline perf checks.

### Bead Pack 08 - 3D Architecture Prep

- 3D subsystem map,
- render abstraction decisions,
- first 3D fixture plan,
- dependency and crate split review.

---

## Suggested Initial Swarm Structure

### Human Roles

- **Lead Architect**: owns plan coherence and architecture decisions.
- **Compatibility Lead**: owns upstream oracle and fixture policy.
- **Runtime Lead**: owns core crate boundaries and lifecycle semantics.
- **Infrastructure Lead**: owns CI, benchmarks, and toolchain stability.

### Agent Lanes

- planning/documentation lane,
- oracle/fixture lane,
- API extraction lane,
- core runtime lane,
- resources/scenes lane,
- rendering/physics lane,
- test/benchmark lane,
- maintenance/refactor lane.

### Initial Scale

Start small enough to preserve coherence.

Suggested initial swarm size:

- **6 to 8 active agents** during early stabilization,
- then expand after bead quality and file boundaries prove stable.

---

## Risks and Mitigations

### Risk: Scope Explosion

**Mitigation**

- enforce `PORT_SCOPE.md`,
- stage milestones aggressively,
- defer editor and broad platform parity.

### Risk: False Compatibility Confidence

**Mitigation**

- require oracle-backed fixtures,
- distinguish visual demos from measured parity,
- preserve golden artifacts in version control.

### Risk: Agent Collisions and Merge Chaos

**Mitigation**

- mandatory reservations,
- bead ownership,
- tighter crate boundaries,
- short-lived branches and frequent sync.

### Risk: Over-Reimplementation of Third-Party Code

**Mitigation**

- require classification before implementation,
- prefer proven ecosystem components where reasonable.

### Risk: Unsafe Rust Sprawl

**Mitigation**

- isolate unsafe code,
- require documented rationale,
- add focused tests and audits.

### Risk: Performance Regression from Early Architecture Choices

**Mitigation**

- capture benchmarks from the first runnable slice,
- make perf visible continuously rather than at the end.

### Risk: Legal/Licensing Mistakes Around Vendored Code

**Mitigation**

- maintain dependency provenance,
- review licenses before port/wrap/reuse decisions,
- document every third-party path.

---

## Success Metrics

### Correctness

- percentage of fixture corpus passing parity tests,
- number of supported scene/resource types,
- signal/notification parity on tracked fixtures,
- render diff pass rate,
- physics trace pass rate.

### Performance

- startup time,
- frame time on representative demos,
- memory footprint,
- resource import/load times,
- build times for core workspace.

### Maintainability

- unsafe-code surface area,
- test coverage of critical crates,
- average bead cycle time,
- merge conflict rate,
- regression escape rate.

### Delivery

- milestone completion trend,
- compatibility matrix growth,
- reproducible demo count,
- release cadence once stabilization begins.

---

## Immediate Next Steps

### Week 1

- create foundation docs,
- pin upstream Godot,
- create Cargo workspace,
- define crate boundaries,
- write first master plan,
- generate first bead graph,
- enable DCG,
- start minimal fixtures.

### Week 2

- build first oracle dumpers,
- stand up `GDExtension` lab,
- extract API coverage data,
- begin `gdvariant` and `gdobject` specs,
- define first headless runtime acceptance tests.

### Week 3+

- implement core runtime subset,
- wire compat tests into CI,
- begin resource and scene execution path,
- prepare first headless milestone report.

---

## First 25 Concrete Tasks

1. Create `AGENTS.md`.
2. Create `PORT_SCOPE.md`.
3. Create `ARCHITECTURE_MAP.md`.
4. Create `COMPAT_MATRIX.md`.
5. Create `TEST_ORACLE.md`.
6. Create `THIRDPARTY_STRATEGY.md`.
7. Add upstream Godot as a pinned submodule.
8. Bootstrap `engine-rs` Cargo workspace.
9. Create empty core crates.
10. Define file reservation policy.
11. Enable destructive-command guard.
12. Define bead template and reporting template.
13. Generate first master plan.
14. Convert master plan into first bead graph.
15. Build minimal scene fixture corpus.
16. Build minimal resource fixture corpus.
17. Write scene-tree oracle dumper.
18. Write property oracle dumper.
19. Write signal/notification tracer.
20. Write resource roundtrip oracle tool.
21. Build API extraction pipeline.
22. Draft `Variant` behavior spec.
23. Draft object lifecycle and signal spec.
24. Create `GDExtension` smoke-test harness.
25. Define headless runtime milestone acceptance tests.

---

## Final Rule Set

- Do not attempt a one-shot rewrite.
- Do not treat the editor as a first milestone.
- Do not translate files blindly.
- Do not let implementation get ahead of test or oracle tooling.
- Do not let multi-agent throughput outrun planning quality.
- Do measure parity from the beginning.
- Do keep crate boundaries strict.
- Do keep `unsafe` narrow and justified.
- Do use upstream Godot as the behavioral oracle.
- Do let Flywheel manage decomposition, routing, and iteration.

---

## One-Sentence Project Thesis

**Build a Rust-native, behavior-compatible Godot runtime in staged vertical slices, using upstream Godot as the oracle and Flywheel as the system that turns a huge rewrite into an executable graph of well-scoped work.**
