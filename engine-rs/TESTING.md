# Patina Engine — Tiered Test Suites

Tests are organized into three tiers by execution time and resource requirements.

## Tier 1 — Fast unit and integration tests (<10s)

Runs all workspace tests excluding golden comparisons and stress tests.

```bash
cargo test --workspace -- --skip golden --skip stress --skip render_golden --skip staleness --skip bench_
```

Use for: local development, pre-commit checks, rapid iteration.

Covers: parsing, scene tree operations, math, signals, object model, input,
scripting, lifecycle notifications, resource loading, cache behavior.

## Tier 2 — Golden comparison tests (~30s)

Runs all workspace tests except stress tests, render goldens, and benchmarks.

```bash
cargo test --workspace -- --skip stress --skip render_golden --skip bench_
```

Use for: CI pull-request checks, verifying golden parity before merge.

Adds to Tier 1: scene golden comparisons, trace parity checks, physics golden
verification, golden staleness/orphan detection, oracle parity.

## Tier 3 — Full suite including stress and render goldens

Runs everything.

```bash
cargo test --workspace
```

Use for: release validation, nightly CI, full regression sweeps.

Adds to Tier 2: render golden image comparison, stress/concurrency tests,
runtime benchmarks.

**CI coverage**: Render golden tests run in two CI paths on every push/PR
to `main` (`.github/workflows/ci.yml`):

1. **`rust` job** — runs `cargo test --workspace` (all tests including render
   goldens) on ubuntu-latest and macos-latest.
2. **`rust-render-goldens` job** — dedicated gate that runs *only* the render
   pixel and golden-image tests via `make test-render` on ubuntu-latest and
   macos-latest. This provides an isolated check so render regressions surface
   as a distinct CI failure rather than hiding inside the full test suite.

Prerequisites: None beyond Rust stable. All render tests use the CPU-based
`SoftwareRenderer` — no GPU, display server, or windowing system required.

Locally, run the same target with:
```bash
cd engine-rs && make test-render
```

## Render pixel tests (CI-safe, headless)

All render tests use the CPU-based `SoftwareRenderer` — no GPU, display server,
or windowing system required. They run identically on headless CI (ubuntu-latest,
macos-latest) and local machines.

| Test file | Tests | CI-safe | Notes |
|---|---|---|---|
| `render_draw_ordering_test.rs` | 40 | Yes | z-index, tree-order, visibility, canvas layers |
| `render_camera_viewport_test.rs` | 26 | Yes | Camera2D zoom/offset/rotation, viewport sizing |
| `render_sprite_property_test.rs` | 29 | Yes | flip_h/v, offset, modulate, centered, texture region |
| `render_golden_test.rs` | 29 | Yes* | Golden image comparison; 2 tests `#[ignore]`d (editor-path hang) |
| `render_vertical_slice_test.rs` | 11 | Yes* | Vertical slice; 1 test `#[ignore]`d (editor-path hang) |

Run all render tests:
```bash
make test-render        # via Makefile
# or directly:
cargo test --workspace -p gdrender2d --test render_draw_ordering_test --test render_camera_viewport_test --test render_sprite_property_test --test render_golden_test --test render_vertical_slice_test
```

## Runtime benchmarks

The `bench_runtime_baselines` test (Tier 3) measures wall-clock time for:
- Stepping 1000 frames on each fixture scene
- Loading + instancing each `.tscn` file (100 iterations)
- Stepping 100 physics frames per scene
- Parsing each `.gd` script file (100 iterations)

Benchmarks always pass — run with `--nocapture` to see timing output:
```bash
cargo test --test bench_runtime_baselines -- --nocapture
```

## Render benchmarks

The `bench_render_baselines` test (Tier 3) measures `SoftwareRenderer::render_frame`
wall-clock time for fixture scenes and synthetic stress tests at three resolutions:

| Resolution | Pixels |
|---|---|
| 640×480 | 0.31 MP |
| 1280×720 | 0.92 MP |
| 1920×1080 | 2.07 MP |

Scenes benchmarked: `space_shooter`, `demo_2d`, `hierarchy`, plus synthetic
stress tests with 100 and 500 canvas items.

Benchmarks always pass — run with `--nocapture` to see timing output:
```bash
cargo test --test bench_render_baselines -- --nocapture
```

## Golden staleness checks

The `golden_staleness_test` (Tier 2) verifies that:
- No golden file is orphaned (unreferenced by any test or tool).
- Scene goldens match regenerated output from source `.tscn` fixtures.
- All golden JSON files are valid JSON.
- All expected golden subdirectories exist and are populated.

If a golden is stale, regenerate it by running the corresponding test with
`--ignored` or re-running the generator (see `tools/oracle/`).

## Editor tests (maintenance-only)

The following test files cover `gdeditor` functionality and are classified as
**maintenance-only**. They exist to catch regressions in existing editor
behavior — not to validate new features. Do not add new feature coverage to
these files until the editor feature gate is lifted (see `AGENTS.md`).

| Test file | Tests | Scope |
|---|---|---|
| `editor_test.rs` | 24 | Integration: server, API, undo/redo, save/load |
| `editor_smoke_test.rs` | 6 | Smoke: server starts, endpoints respond, round-trips work |
| `gdeditor` unit tests | 267 | Crate internals |

## Naming conventions

- Tests containing `golden` or `staleness` in their name are Tier 2.
- Tests containing `stress`, `render_golden`, or `bench_` in their name are Tier 3.
- All other tests are Tier 1.
