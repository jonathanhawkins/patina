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

**CI coverage**: The `rust` job in `.github/workflows/ci.yml` runs
`cargo test --workspace` on every push/PR to `main`, which includes all
render golden tests (`tests/render_golden_test.rs`). These tests verify
texture drawing, camera/viewport parity, draw ordering, visibility, layer
semantics, determinism, and golden image comparison. No additional CI
configuration is needed — render goldens are covered by the existing workflow.

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

## Golden staleness checks

The `golden_staleness_test` (Tier 2) verifies that:
- No golden file is orphaned (unreferenced by any test or tool).
- Scene goldens match regenerated output from source `.tscn` fixtures.
- All golden JSON files are valid JSON.
- All expected golden subdirectories exist and are populated.

If a golden is stale, regenerate it by running the corresponding test with
`--ignored` or re-running the generator (see `tools/oracle/`).

## Naming conventions

- Tests containing `golden` or `staleness` in their name are Tier 2.
- Tests containing `stress`, `render_golden`, or `bench_` in their name are Tier 3.
- All other tests are Tier 1.
