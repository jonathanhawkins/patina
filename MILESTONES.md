# MILESTONES.md - Phase Plan with Deliverables

This document defines the phased delivery plan for the Patina Engine, with objectives, deliverables, and exit criteria for each phase.

---

## Phase Overview

| Phase | Name | Focus | Estimated Duration |
|-------|------|-------|--------------------|
| 0 | Foundation and Planning | Planning system, scope, repo structure, bead graph | 1-2 weeks |
| 1 | Oracle, Fixtures, and Contracts | Upstream oracle, fixture corpus, API contracts | 2-3 weeks |
| 2 | GDExtension Compatibility Lab | Rust inside Godot, assumption validation | 2-3 weeks |
| 3 | Headless Rust Runtime | Core types, object model, scene execution | 4-6 weeks |
| 4 | 2D Vertical Slice | First end-to-end graphical milestone | 4-6 weeks |
| 5 | Broader Runtime and 3D Prep | Expanded coverage, audio, 3D groundwork | 4-6 weeks |
| 6 | 3D Runtime Slice | First 3D milestone | 6-8 weeks |
| 7 | Platform Layer and Distribution | Windowing, input, packaging, CI | 4-6 weeks |
| 8 | Editor-Facing Work | Editor APIs, tooling hooks, inspectors | 6-8 weeks |
| 9 | Hardening and Release | Benchmarks, fuzz testing, release train | Ongoing |

---

## Phase 0: Foundation and Planning

### Objectives

- Build the planning and execution infrastructure.
- Define project scope and architecture.
- Establish the repository structure and workspace.
- Produce the master plan and convert it into beads.

### Deliverables

- [ ] All foundation documents created (AGENTS.md, PORT_SCOPE.md, ARCHITECTURE_MAP.md, COMPAT_MATRIX.md, RISK_REGISTER.md, TEST_ORACLE.md, BENCHMARKS.md, THIRDPARTY_STRATEGY.md, CRATE_BOUNDARIES.md, MILESTONES.md)
- [x] Upstream Godot added as pinned submodule
- [ ] Rust workspace bootstrapped with empty core crates
- [ ] First master plan drafted
- [ ] Bead graph created with first 50-100 beads prioritized
- [ ] Agent roles and swarm composition defined
- [ ] DCG enabled
- [ ] File reservation policy live
- [ ] Reporting format defined

### Exit Criteria

- No major coding starts without corresponding beads.
- Core documents are reviewed and internally consistent.
- First 50-100 beads are prioritized in the bead graph.
- First swarm run can proceed without ambiguity.

---

## Phase 1: Oracle, Fixtures, and Contracts

### Objectives

- Turn upstream Godot into a measurable behavioral oracle.
- Build the fixture corpus for all supported fixture classes.
- Extract and normalize API contracts from upstream.

### Deliverables

- [ ] Fixture corpus for scenes, resources, rendering, and physics
- [ ] Scene tree dumper (runs inside upstream Godot)
- [ ] Property dumper
- [ ] Signal/notification tracer
- [ ] Resource roundtrip tool
- [ ] API extraction pipeline
- [ ] Golden output format defined and implemented
- [ ] Compatibility dashboard (basic version)

### Exit Criteria

- Upstream outputs can be generated automatically from fixture definitions.
- Fixture behavior is versioned and reproducible.
- Contracts are available to implementation teams in a normalized format.
- Golden outputs are stored in version control under `fixtures/golden/`.

---

## Phase 2: GDExtension Compatibility Lab

### Objectives

- Use Rust inside real Godot (via GDExtension and godot-rust) to validate compatibility assumptions before deeper runtime replacement.
- Build diagnostic and compatibility helpers.

### Deliverables

- [ ] GDExtension harness (apps/godot/)
- [ ] godot-rust smoke-test modules
- [ ] Scene and resource inspectors (Rust running inside Godot)
- [ ] Signal/notification tracing helpers
- [ ] API coverage tooling (what percentage of API is exercised by fixtures)

### Exit Criteria

- Rust tools can run inside upstream Godot reliably.
- Major contract misunderstandings are identified and documented.
- Fixture generation and runtime inspection are stable.
- Assumptions about object lifecycle, signals, and resource behavior are validated.

---

## Phase 3: Headless Rust Runtime

### Objectives

- Build the first independent Rust runtime slice without rendering dependency.
- Implement core types, object model, signals, notifications, resource loading, and scene execution.

### Deliverables

- [ ] `gdcore` first working version (math types, IDs, strings, errors)
- [ ] `gdvariant` first working version (Variant enum, conversions, serialization)
- [ ] `gdobject` first working version (object model, signals, notifications, refcounting)
- [ ] `gdresource` first working version (resource loading, .tres parsing, cache)
- [ ] `gdscene` first working version (Node, SceneTree subset, lifecycle)
- [ ] Headless runner (execute scenes without rendering)
- [ ] Compatibility tests for simple scene execution
- [ ] Resource roundtrip tests
- [ ] Signal ordering tests
- [ ] Notification dispatch tests

### Exit Criteria

- Simple fixtures load and execute in the Rust runtime.
- Object lifecycle semantics (enter, ready, process, exit) are stable and tested.
- Parity tests pass for the agreed fixture set (scene, signal, property, resource categories).
- No rendering required; all tests run headless.

---

## Phase 4: 2D Vertical Slice

### Objectives

- Deliver the first meaningful end-to-end graphical milestone.
- Demonstrate a working 2D rendering path with input handling.

### Deliverables

- [ ] `gdserver2d` initial implementation (2D rendering server API)
- [ ] `gdrender2d` initial implementation (2D rendering backend)
- [ ] `gdphysics2d` initial implementation (basic 2D physics)
- [ ] 2D node subset (Node2D, Sprite2D, Camera2D, CanvasItem)
- [ ] Transform2D hierarchy working
- [ ] Input subset working (keyboard, mouse basics)
- [ ] Frame loop (timing, process, physics process)
- [ ] Render snapshot tests against upstream golden outputs
- [ ] Simple 2D demo project compatibility
- [ ] Baseline performance measurements (BENCHMARKS.md populated)

### Exit Criteria

- At least one real 2D project or representative fixture set runs end-to-end.
- Render outputs stay within agreed diff thresholds compared to upstream.
- Frame loop correctness is testable and repeatable.
- Performance baselines are established and documented.

---

## Phase 5: Broader Runtime and 3D Prep

### Objectives

- Expand runtime coverage after the 2D slice is stable.
- Begin groundwork for 3D support.

### Deliverables

- [ ] Richer resource types supported
- [ ] `gdaudio` initial implementation (basic audio playback)
- [ ] Broader input handling (gamepad, touch basics)
- [ ] Scene instancing edge cases resolved
- [ ] Improved compatibility matrix (COMPAT_MATRIX.md updated with measured status)
- [ ] Broader integration fixtures
- [ ] First audio test harness
- [ ] Initial 3D architecture spec and crate plan

### Exit Criteria

- Core runtime no longer depends on milestone-specific hacks or shortcuts.
- Broadened scene and resource support is measurable via compatibility tests.
- Audio basics work and are testable.
- 3D work can begin on a stable foundation.

---

## Phase 6: 3D Runtime Slice

### Objectives

- Deliver the first meaningful 3D milestone.
- Validate the rendering and physics architecture for 3D workloads.

### Deliverables

- [ ] First 3D crate set (gdserver3d, gdrender3d, gdphysics3d or extensions of existing crates)
- [ ] 3D node subset (Node3D, Camera3D, MeshInstance3D, DirectionalLight3D)
- [ ] 3D transform hierarchy
- [ ] Initial 3D render path
- [ ] Initial 3D physics hooks
- [ ] 3D fixture corpus
- [ ] Render and physics comparison tooling for 3D
- [ ] First real 3D demo parity report

### Exit Criteria

- Representative 3D fixtures run and produce comparable output.
- Performance and correctness are measurable.
- Platform and runtime boundaries remain clean (3D does not destabilize 2D).

---

## Phase 7: Platform Layer and Distribution

### Objectives

- Harden the runtime for real-world deployment.
- Build platform abstraction and packaging.

### Deliverables

- [ ] `gdplatform` first stable layer
- [ ] Desktop platform targets (Linux, macOS, Windows)
- [ ] Window creation and management
- [ ] Full input handling (keyboard, mouse, gamepad)
- [ ] Timing and frame synchronization
- [ ] Startup and runtime packaging flow
- [ ] CI matrix for all supported target platforms
- [ ] Artifact generation (release binaries)

### Exit Criteria

- Runtime can be built and run in a repeatable way across all initial target platforms.
- Platform-specific code is isolated in `gdplatform` and platform backends.
- CI produces artifacts for all targets.

---

## Phase 8: Editor-Facing Work

### Objectives

- Approach editor support only after runtime foundations are stable.
- Build editor compatibility layer without destabilizing the runtime.

### Deliverables

- [ ] Editor architecture plan
- [ ] `gdeditor` initial implementation
- [ ] Editor APIs (inspector, scene editor hooks)
- [ ] Tooling hooks for import pipeline
- [ ] Selected tooling parity milestones (inspector, scene tree editor)
- [ ] Partial editor feature set

### Exit Criteria

- Editor work does not destabilize runtime milestones.
- Runtime-first architecture remains intact.
- At least basic inspector and scene editing functionality works.

---

## Phase 9: Hardening and Release

### Objectives

- Turn milestone success into sustained project health.
- Establish a repeatable release process.

### Deliverables

- [ ] Benchmark dashboards (automated, historical)
- [ ] Fuzz testing and property-based tests for critical subsystems
- [ ] Crash triage process defined
- [ ] Release train established (cadence, branching, versioning)
- [ ] Contributor onboarding documentation
- [ ] Migration guide for users moving projects from Godot to Patina

### Exit Criteria

- Repeatable release cadence is operational.
- Stable regression suite catches breakages before release.
- Known-risk backlog is clearly owned and tracked.
- New contributors can onboard and contribute within a reasonable timeframe.

---

## Cross-Phase Principles

1. **No phase starts without the previous phase's exit criteria being met** (with the exception of overlapping planning work).
2. **Every phase produces a usable vertical slice** -- not just internal infrastructure.
3. **Tests and oracle tooling must keep pace with implementation** -- implementation cannot get ahead of test coverage.
4. **Compatibility is measured, not assumed** -- every phase updates COMPAT_MATRIX.md with actual test results.
5. **Performance is tracked from Phase 4 onward** -- BENCHMARKS.md is updated each phase.
