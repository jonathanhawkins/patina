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

### Runtime benchmark baselines (Godot 4.6.1 pin, 2026-03-20)

Measured on macOS (Apple Silicon), debug build.

| Benchmark | Time | Per-unit |
|---|---|---|
| load+instance minimal | 2.98ms / 100x | 0.030 ms/iter |
| load+instance hierarchy | 6.64ms / 100x | 0.066 ms/iter |
| load+instance space_shooter | 11.54ms / 100x | 0.115 ms/iter |
| load+instance platformer | 13.38ms / 100x | 0.134 ms/iter |
| load+instance physics_playground | 15.61ms / 100x | 0.156 ms/iter |
| step 1000 frames minimal | 11.08ms | 0.011 ms/frame |
| step 1000 frames hierarchy | 39.35ms | 0.039 ms/frame |
| step 1000 frames space_shooter | 45.16ms | 0.045 ms/frame |
| step 1000 frames physics_playground | 66.02ms | 0.066 ms/frame |
| step 1000 frames platformer | 111.65ms | 0.112 ms/frame |
| physics 100 frames space_shooter | 4.75ms | 0.048 ms/frame |
| physics 100 frames physics_playground | 5.65ms | 0.056 ms/frame |
| physics 100 frames platformer | 10.37ms | 0.104 ms/frame |
| parse test_move.gd | 3.71ms / 100x | 0.037 ms/iter |
| parse enemy_spawner.gd | 4.29ms / 100x | 0.043 ms/iter |
| parse test_movement.gd | 5.14ms / 100x | 0.051 ms/iter |
| parse test_variables.gd | 6.92ms / 100x | 0.069 ms/iter |
| parse player.gd | 10.16ms / 100x | 0.102 ms/iter |
| variant_roundtrip | 1.23ms / 100x | 0.012 ms/iter |

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

### Render benchmark baselines (Godot 4.6.1 pin, 2026-03-20)

Measured on macOS (Apple Silicon), debug build, SoftwareRenderer.

| Scene | 640x480 | 1280x720 | 1920x1080 |
|---|---|---|---|
| space_shooter | 0.988 ms (311 MP/s) | 3.008 ms (306 MP/s) | 6.896 ms (301 MP/s) |
| hierarchy | 0.976 ms (315 MP/s) | 3.130 ms (294 MP/s) | 6.816 ms (304 MP/s) |
| demo_2d | 1.018 ms (302 MP/s) | 3.206 ms (287 MP/s) | 6.911 ms (300 MP/s) |
| stress_100_items | 2.858 ms (108 MP/s) | 8.673 ms (106 MP/s) | 17.499 ms (119 MP/s) |
| stress_500_items | 3.222 ms (95 MP/s) | 9.604 ms (96 MP/s) | 20.641 ms (101 MP/s) |

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
| `editor_test.rs` | 53 | Integration: server, API, undo/redo, save/load |
| `editor_smoke_test.rs` | 6 | Smoke: server starts, endpoints respond, round-trips work |
| `gdeditor` unit tests | 326 | Crate internals (1 skipped: `debug_hierarchy_hang_repro`) |

## Oracle regeneration (manual, not automated in CI)

Oracle outputs (fixtures, golden JSON) are committed to the repo and validated
by CI — but never auto-regenerated by CI. Regeneration is a manual local step.

**When to regenerate**: after a Godot repin, behavioral change, or oracle script
update.

**How to regenerate**:

```bash
# From repo root
tools/oracle/run_all.sh

# Review all diffs before committing:
git diff fixtures/ tests/golden/
```

Commit the updated outputs alongside the triggering code change. CI will
validate them on the next push.

See `.github/workflows/ci.yml` for the comment block explaining this policy.

## Naming conventions

- Tests containing `golden` or `staleness` in their name are Tier 2.
- Tests containing `stress`, `render_golden`, or `bench_` in their name are Tier 3.
- All other tests are Tier 1.
