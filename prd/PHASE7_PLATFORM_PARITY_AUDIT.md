# Phase 7 Platform Parity Audit

Date: 2026-03-29
Target upstream: Godot `4.6.1-stable`
Patina phase: `Phase 7 - Platform Layer and Distribution`

## Purpose

This document turns Phase 7 from a broad platform milestone into a parity audit
for the supported desktop/runtime layer.

It answers four questions:

1. What platform/runtime behavior does Godot expose in the Phase 7 scope?
2. What does Patina currently implement and measure?
3. Where do Patina docs overclaim relative to measured evidence?
4. Which remaining gaps should become beads without duplicating existing work?

## Audit Rules

Use this workflow for all future Phase 7 parity work.

1. Scope only the supported desktop/runtime layer.
2. Treat upstream Godot desktop/runtime behavior as the behavioral source.
3. Classify each audited family as one of:
   - `Measured`
   - `Implemented, not yet measured`
   - `Deferred`
   - `Missing`
4. Do not create a new bead if an active or closed bead already covers the
   same measurable outcome.
5. Prefer one bead per behavior cluster, not one bead per OS file.
6. Keep headless contract coverage distinct from native OS parity coverage.

## Sources To Compare

### Upstream Godot

Primary upstream behavior families for this phase:

- OS singleton / timing / environment behavior
- DisplayServer window lifecycle and display state
- desktop target and export/preset behavior
- startup/bootstrap/runtime lifecycle

Relevant supporting areas are the upstream display, OS, window, and export
surfaces rather than the entire editor/export pipeline.

### Patina

Primary local crates:

- `engine-rs/crates/gdplatform/`
- `engine-rs/crates/patina-runner/`
- `engine-rs/crates/gdscene/` for MainLoop/bootstrap integration

Primary local evidence:

- `docs/PLATFORM_STABLE_LAYER.md`
- `docs/migration-guide.md`
- `engine-rs/tests/platform_first_stable_layer_test.rs`
- `engine-rs/tests/platform_targets_validation_test.rs`
- `engine-rs/tests/ci_build_matrix_platform_test.rs`
- `engine-rs/tests/startup_runtime_packaging_flow_test.rs`
- `engine-rs/tests/startup_packaging_ci_gate_test.rs`
- `engine-rs/tests/window_lifecycle_test.rs`
- `engine-rs/tests/window_lifecycle_parity_test.rs`
- `engine-rs/tests/window_creation_abstraction_test.rs`
- `engine-rs/tests/linux_x11_wayland_platform_test.rs`
- `engine-rs/tests/macos_platform_layer_test.rs`
- `engine-rs/tests/windows_platform_layer_test.rs`
- `engine-rs/tests/windows_platform_win32_backend_test.rs`
- `engine-rs/tests/web_wasm_export_interop_test.rs`

## Current Patina Phase 7 Read

Phase 7 is more real than the old milestone wording suggests.

Patina already has:

- a stable `gdplatform` layer spec
- explicit desktop target metadata
- headless and trait-based backend composition
- packaging/export config types and executor
- startup/bootstrap integration tests
- dedicated Linux, macOS, and Windows platform-layer tests
- CI-matrix tests that assert multi-OS workflow coverage

The key audit problem is not “whether Phase 7 exists”.

The real question is how much of this is:

- measured runtime/platform coverage
- contract/spec coverage in headless mode
- native OS parity versus API/data-model scaffolding

## Claim Mismatch: Docs vs Measured Evidence

The migration guide currently presents Phase 7 as:

- desktop targets
- runtime capability queries
- export/package configuration

That is directionally correct, but it reads more like a completed user-facing
surface than a bounded measured slice.

Two areas need especially careful wording:

1. Native OS platform behavior
   The repo has substantial Linux/macOS/Windows tests, but much of that local
   coverage is headless model behavior and platform-API semantics rather than
   proof of live native-window parity against Godot.

2. Packaging/export behavior
   Patina has a meaningful local packaging executor and export config flow, but
   that is not the same thing as full Godot export-template parity or full
   downstream app-distribution parity.

## Initial Phase 7 Classification

This is the first audit pass, not the final matrix.

### First Matrix Rows

| Upstream Family | Patina Area | Current Status | Evidence | Gap Type | Existing Bead | Action |
|-----------------|-------------|----------------|----------|----------|---------------|--------|
| headless backend and platform trait composition | `gdplatform::backend`, `gdplatform::window`, `gdplatform::display` | Measured | `platform_first_stable_layer_test.rs`, `startup_packaging_ci_gate_test.rs` | none | `pat-yzdv7` overlaps stable-layer scope | reuse evidence, avoid duplicate implementation beads |
| desktop target registry and target validation | `gdplatform::platform_targets` | Measured for declared target metadata | `platform_targets_validation_test.rs`, `ci_build_matrix_platform_test.rs` | missing breadth | `pat-2uc5z` | reuse; only add if target claims expand |
| startup/bootstrap through runtime loop | `patina_runner::bootstrap`, `gdscene::MainLoop` | Measured for headless repeatable startup flow | `startup_runtime_packaging_flow_test.rs`, `startup_packaging_ci_gate_test.rs` | missing native breadth | `pat-vjmfv` overlaps lifecycle path | reuse evidence |
| packaging/export config and staging pipeline | `gdplatform::export` | Measured for Patina packaging flow | `startup_runtime_packaging_flow_test.rs` | docs-overclaim | `pat-vjmfv` | keep scoped to Patina packaging flow, not full Godot export parity |
| CI matrix for supported desktop platforms | `.github/workflows/ci.yml`, `gdplatform::platform_targets` | Measured as repo policy | `ci_build_matrix_platform_test.rs` | none | `pat-s3700`, `pat-2uc5z`, `pat-vjmfv` | reuse existing beads |
| generic window lifecycle abstraction | `gdplatform::window`, `gdplatform::display` | Measured for headless/backend abstraction | `window_creation_abstraction_test.rs`, `window_lifecycle_test.rs`, `window_lifecycle_parity_test.rs` | missing native breadth | historical windowing work, `pat-yzdv7` | add measurement only if live native parity enters scope |
| Linux platform layer | `gdplatform::linux` | Implemented, partly measured | `linux_x11_wayland_platform_test.rs` | missing-test / docs-overclaim | none active specific | classify as bounded Linux protocol/model coverage |
| macOS platform layer | `gdplatform::macos` | Implemented, partly measured | `macos_platform_layer_test.rs` | missing-test / docs-overclaim | none active specific | classify as bounded macOS API/model coverage |
| Windows platform layer | `gdplatform::windows` | Implemented, partly measured | `windows_platform_layer_test.rs`, `windows_platform_win32_backend_test.rs` | missing-test / docs-overclaim | none active specific | classify as bounded Windows API/model coverage |
| Web/WASM export surface | `gdplatform::web`, target metadata | Deferred / limited | `web_wasm_export_interop_test.rs`, migration guide limitation section | deferred | none | keep explicitly outside supported desktop slice |

### Stable Layer / Startup Notes

#### Headless stable layer composition

- Patina evidence:
  - `docs/PLATFORM_STABLE_LAYER.md`
  - `engine-rs/tests/platform_first_stable_layer_test.rs`
  - `engine-rs/tests/startup_packaging_ci_gate_test.rs`
- Current classification: `Measured`
- Reason:
  - The repo has strong direct evidence for headless platform initialization,
    trait-object backend isolation, event routing, timers, target validation,
    thread primitives, and deterministic construction.
  - This is real platform-layer coverage, not just design prose.
  - It remains bounded to headless and abstracted backend behavior.

#### Bootstrap / runtime startup flow

- Patina evidence:
  - `engine-rs/tests/startup_runtime_packaging_flow_test.rs`
  - `engine-rs/tests/startup_packaging_ci_gate_test.rs`
- Current classification: `Measured for repeatable headless startup`
- Reason:
  - Bootstrap ordering, frame-loop integration, shutdown, and packaging flow
    are exercised as a single lifecycle.
  - This supports a real “repeatable startup/runtime path” claim.
  - It does not prove native per-OS app bundle/runtime behavior beyond the
    headless and staging pipeline slice.

### Desktop Target / CI Notes

#### Desktop target registry

- Patina evidence:
  - `engine-rs/crates/gdplatform/src/platform_targets.rs`
  - `engine-rs/tests/platform_targets_validation_test.rs`
  - `engine-rs/tests/ci_build_matrix_platform_test.rs`
- Current classification: `Measured for declared target metadata`
- Reason:
  - Linux, macOS, Windows, and WASM target rows are explicit and heavily tested
    as metadata and CI policy.
  - The strongest supported claim here is “Patina has a defined and validated
    supported-target matrix”, not “every target has full runtime parity”.

#### CI matrix coverage

- Patina evidence:
  - `.github/workflows/ci.yml`
  - `engine-rs/tests/ci_build_matrix_platform_test.rs`
- Current classification: `Measured as repo policy`
- Reason:
  - The repo tests that the workflow contains Linux/macOS/Windows coverage,
    caching, release checks, and matrix behavior.
  - This is strong evidence for CI policy and validation coverage, not for
    user-facing distribution parity by itself.

### Packaging / Export Notes

#### Export config and template generation

- Patina evidence:
  - `engine-rs/crates/gdplatform/src/export.rs`
  - `engine-rs/tests/startup_runtime_packaging_flow_test.rs`
  - `engine-rs/tests/export_template_debug_release_test.rs`
- Current classification: `Measured for Patina packaging flow`
- Reason:
  - Config objects, template generation, manifest output, resource collection,
    and output artifact staging are all exercised locally.
  - This is enough to claim a Patina packaging flow.
  - It is not enough to claim full Godot export preset/template parity.

### Native Platform Layer Notes

This is the highest-risk overclaim area in Phase 7.

#### Linux platform layer

- Patina evidence:
  - `engine-rs/crates/gdplatform/src/linux.rs`
  - `engine-rs/tests/linux_x11_wayland_platform_test.rs`
- Current classification: `Implemented, partly measured`
- Reason:
  - Protocol detection, desktop-environment flags, compositing, IME, cursor
    configuration, and headless defaults are all tested.
  - The evidence is strongest for local Linux model/API semantics.
  - It is weaker for true live X11/Wayland runtime parity under native windowed
    execution.

#### macOS platform layer

- Patina evidence:
  - `engine-rs/crates/gdplatform/src/macos.rs`
  - `engine-rs/tests/macos_platform_layer_test.rs`
- Current classification: `Implemented, partly measured`
- Reason:
  - Menu bar, app menu, dock badge, theme/accessibility flags, and action
    routing are covered in headless/native-model tests.
  - This is useful and substantial.
  - It still falls short of broad live macOS shell parity.

#### Windows platform layer

- Patina evidence:
  - `engine-rs/crates/gdplatform/src/windows.rs`
  - `engine-rs/tests/windows_platform_layer_test.rs`
  - `engine-rs/tests/windows_platform_win32_backend_test.rs`
- Current classification: `Implemented, partly measured`
- Reason:
  - DPI awareness, display info, taskbar integration, backend delegation, and
    lifecycle behavior are covered in focused tests.
  - That is stronger than a placeholder implementation.
  - It remains bounded local coverage rather than full Win32 runtime parity.

### Deferred or Explicitly Limited

These should remain outside the supported Phase 7 desktop parity slice unless
project scope changes:

- mobile export/runtime parity
- full web platform parity
- full Godot export-template parity
- full native shell integration parity for every OS behavior surface

## Existing Beads To Reuse

Do not create duplicates for these active Phase 7 beads:

- `pat-2uc5z` Define supported desktop platform targets and validation coverage
- `pat-vjmfv` Add startup packaging flow and supported-target CI matrix
- `pat-yzdv7` Stabilize gdplatform windowing input and timing layer

Related live bead:

- `pat-s3700` CI matrix for supported targets — guards the target matrix
  representation in CI and the doc-validation test that protects expected
  target entries

Any new Phase 7 bead must answer:

1. Why do the current target/startup/stable-layer beads not already cover it?
2. Is the gap about native platform parity, docs alignment, or missing tests?
3. What exact command, test, or artifact proves it done?

## Bead Candidates From This Audit

These are the first non-duplicative candidate beads.

### Candidate 1

Title:
`Phase 7 audit: reconcile platform-layer support claims with measured evidence`

Acceptance:

- Phase 7 rows in `docs/migration-guide.md`, `COMPAT_MATRIX.md`, and related
  docs distinguish `Measured`, `Implemented, not yet measured`, `Deferred`, and
  `Missing`
- docs no longer imply full native platform parity where only headless or
  model/API coverage exists

### Candidate 2

Title:
`Phase 7 parity: classify native OS platform-layer coverage versus headless stable-layer coverage`

Acceptance:

- Linux, macOS, and Windows platform-layer surfaces are each classified as
  headless/model coverage, native-runtime coverage, or deferred
- the matrix cites concrete tests for each supported claim
- no doc conflates trait/headless coverage with live native-shell parity

### Candidate 3

Title:
`Phase 7 parity: keep packaging-flow claims scoped to Patina staging and startup artifacts`

Acceptance:

- packaging/export docs explicitly state what Patina's current executor covers
- claims do not imply full Godot export-template parity
- a doc-validation or focused test guards the scoped wording if needed

## Supported Desktop Targets

This section is the canonical per-target summary required by bead `pat-2uc5z`.
It names each supported desktop target, its measured versus claimed coverage,
and the validation evidence backing each claim.

### Target Matrix

| Target | Triple | CI Tested | Measured Coverage | Claimed Coverage | Validation Evidence |
|--------|--------|-----------|-------------------|------------------|---------------------|
| Linux x86_64 | `x86_64-unknown-linux-gnu` | Yes | Headless backend, platform trait composition, X11/Wayland protocol model, event routing, timers, startup lifecycle | Desktop platform layer with windowing and GPU | `platform_first_stable_layer_test.rs`, `linux_x11_wayland_platform_test.rs`, `platform_targets_validation_test.rs`, `startup_runtime_packaging_flow_test.rs` |
| Linux aarch64 | `aarch64-unknown-linux-gnu` | No | Target metadata only (not CI-exercised) | Same as Linux x86_64 | `platform_targets_validation_test.rs` (metadata only) |
| macOS x86_64 | `x86_64-apple-darwin` | Yes | Headless backend, menu bar/dock/theme model, action routing, startup lifecycle | Desktop platform layer with windowing and GPU | `platform_first_stable_layer_test.rs`, `macos_platform_layer_test.rs`, `platform_targets_validation_test.rs`, `startup_runtime_packaging_flow_test.rs` |
| macOS aarch64 | `aarch64-apple-darwin` | Yes | Same as macOS x86_64 (native CI runner) | Desktop platform layer with windowing and GPU | Same as macOS x86_64 |
| Windows x86_64 | `x86_64-pc-windows-msvc` | Yes | Headless backend, DPI/taskbar/display model, Win32 backend delegation, startup lifecycle | Desktop platform layer with windowing and GPU | `platform_first_stable_layer_test.rs`, `windows_platform_win32_backend_test.rs`, `windows_platform_layer_test.rs`, `platform_targets_validation_test.rs`, `startup_runtime_packaging_flow_test.rs` |
| Windows aarch64 | `aarch64-pc-windows-msvc` | No | Target metadata only (not CI-exercised) | Same as Windows x86_64 | `platform_targets_validation_test.rs` (metadata only) |
| Web (WASM) | `wasm32-unknown-unknown` | No | Export interop model only | Limited / deferred | `web_wasm_export_interop_test.rs` |

### Coverage Classification Per Target

**Tier 1 — CI-tested, measured headless + model coverage:**
- Linux x86_64
- macOS x86_64
- macOS aarch64 (Apple Silicon)
- Windows x86_64

These targets have direct CI coverage and integration tests exercising the
headless backend, platform-specific model APIs, and startup lifecycle. The
measured evidence is strongest for headless/trait-based behavior and
platform-API semantics. Live native-window parity against Godot is not yet
proven for any target.

**Tier 2 — Declared, not CI-tested:**
- Linux aarch64
- Windows aarch64

These targets exist in the registry with correct metadata and export config
generation, but are not exercised in CI. Their runtime behavior is assumed
equivalent to the x86_64 variant of the same OS.

**Tier 3 — Deferred / limited:**
- Web (WASM)

Exists as a target definition for export interop testing. No GPU, no windowing,
no native runtime parity claim.

### What "Supported" Means

A target being "supported" in Phase 7 means:

1. It is registered in `DESKTOP_TARGETS` with correct metadata
2. `validate_current_target()` succeeds when built for that triple
3. Export configs can be generated for it
4. Platform capabilities are correctly reported

For Tier 1 targets, it additionally means:

5. The headless backend initializes correctly
6. Platform-specific model tests pass in CI
7. The startup/bootstrap lifecycle completes

It does **not** mean full native Godot DisplayServer parity or live windowed
runtime equivalence.

## Startup and Packaging Flow

This section documents Patina's current startup and packaging artifact path,
scoped to what the Phase 7 audit actually measures.

### Startup Lifecycle

Patina's startup follows an 8-phase bootstrap sequence modeled on Godot's
initialization order:

1. **Core** — math types, Variant system, OS singleton
2. **Servers** — ClassDB registration, PhysicsServer, RenderingServer
3. **Resources** — ResourceLoader, resource cache, importers
4. **SceneTree** — SceneTree creation, root viewport
5. **MainScene** — load and instance the project's main scene
6. **Scripts** — parse and attach GDScript instances to nodes
7. **Lifecycle** — enter_tree / _ready notifications in tree order
8. **Running** — begin frame stepping (_process / _physics_process)

Implementation: `engine-rs/crates/patina-runner/src/bootstrap.rs`
Evidence: `engine-rs/tests/startup_runtime_packaging_flow_test.rs`

The startup flow is measured for **headless repeatable execution**. All 8 phases
complete in CI. Native per-OS app bundle startup is not yet measured.

### Packaging Artifact Path

Patina's packaging pipeline produces staging artifacts, not native executables.
The current artifact path is:

1. **Config** — `ExportConfig` specifies target platform, build profile, app
   name, icon, and resource paths
2. **Validate** — `PackageExecutor::validate_platform()` checks the target
   against `DESKTOP_TARGETS` (linux, macos, windows, web only)
3. **Collect** — `PackageExecutor::validate_and_collect()` resolves `res://`
   paths from the project directory, collecting file metadata
4. **Stage** — `PackageExecutor::run()` writes three artifacts to the output
   directory:
   - `export_manifest.txt` — app name, platform, profile, resource count, total
     size
   - `resource_list.txt` — tab-separated listing of package paths, source paths,
     and sizes
   - `<AppName>.<platform>.<profile>.<arch>` — output marker (staging
     placeholder, not a native binary)

Implementation: `engine-rs/crates/gdplatform/src/export.rs`
Evidence: `engine-rs/tests/startup_runtime_packaging_flow_test.rs`

### What This Path Does NOT Cover

The packaging artifact path is scoped to Patina's staging pipeline. It does
**not** cover:

- Godot export preset/template parity
- Native binary generation or linking
- App bundle creation (macOS .app, Windows installer, Linux AppImage)
- Code signing or notarization
- Downstream app-store distribution
- Runtime resource compression or encryption

These are outside the Phase 7 measured slice unless project scope changes.

### Validation

The startup and packaging flow is exercised end-to-end by:

- `engine-rs/tests/startup_runtime_packaging_flow_test.rs` — bootstrap → run →
  package → verify artifacts (17 tests)
- `engine-rs/tests/startup_packaging_ci_gate_test.rs` — CI gate for the
  startup/packaging flow
- `engine-rs/tests/phase7_startup_packaging_doc_validation_test.rs` —
  doc-validation test guarding this section

## Instructions For Continuing This Audit

Follow this order:

1. Build the matrix from behavior families, not crate/module names.
2. Map evidence before keeping any “supported” claim.
3. Reconcile docs before opening new implementation beads.
4. Open new beads only for clearly missing parity evidence or real missing
   implementation not already covered by current Phase 7 work.

Recommended row format:

| Family | Patina Area | Current Status | Evidence | Gap Type | Existing Bead | Action |
|--------|-------------|----------------|----------|----------|---------------|--------|

Where:

- `Gap Type` is one of `docs-overclaim`, `missing-test`, `missing-impl`, `deferred`
- `Existing Bead` must be checked before proposing any new bead
- `Action` should be `reuse`, `narrow docs`, `add measurement`, or `new bead`

## Immediate Next Step

The next useful implementation step is not another broad platform milestone bead.

It is to reconcile Phase 7 docs against this matrix, especially:

- what “supported desktop targets” really means today
- what the packaging flow does and does not cover
- what native Linux/macOS/Windows coverage is actually measured
