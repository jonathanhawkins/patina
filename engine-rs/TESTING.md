# Patina Engine — Tiered Test Suites

Tests are organized into three tiers by execution time and resource requirements.

## Tier 1 — Fast unit and integration tests (<10s)

Runs all workspace tests excluding golden comparisons and stress tests.

```bash
cargo test --workspace -- --skip golden --skip stress --skip render_golden --skip staleness
```

Use for: local development, pre-commit checks, rapid iteration.

Covers: parsing, scene tree operations, math, signals, object model, input,
scripting, lifecycle notifications, resource loading, cache behavior.

## Tier 2 — Golden comparison tests (~30s)

Runs all workspace tests except stress tests and render goldens.

```bash
cargo test --workspace -- --skip stress --skip render_golden
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
benchmark fixtures.

## Golden staleness checks

The `golden_staleness_test` (Tier 2) verifies that:
- No golden file is orphaned (unreferenced by any test or tool).
- Scene goldens match regenerated output from source `.tscn` fixtures.
- All golden JSON files are valid JSON.
- All expected golden subdirectories exist and are populated.

If a golden is stale, regenerate it by running the corresponding test with
`--ignored` or re-running the generator (see `tools/oracle/`).

## Naming conventions

- Tests containing `golden` in their name are Tier 2.
- Tests containing `stress` in their name are Tier 3.
- Tests containing `render_golden` in their name are Tier 3.
- Tests containing `staleness` in their name are Tier 2.
- All other tests are Tier 1.
