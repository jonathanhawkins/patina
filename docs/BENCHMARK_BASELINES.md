# Benchmark Baselines

This document records performance baselines for the Patina Engine benchmark suite.
Baselines are versioned against the upstream Godot oracle pin so that regressions
can be detected and improvements attributed to the correct delta.

---

## How to Run

```sh
cd engine-rs
cargo run --example benchmarks
```

Output is JSON. Pipe through `jq` for pretty printing:

```sh
cargo run --example benchmarks | jq .
```

Benchmark source: `engine-rs/examples/benchmarks.rs`

Each benchmark runs **100 iterations** (plus 1 warm-up). Reported times are
microseconds (µs) measured with `std::time::Instant` in an unoptimized debug
build (`cargo run`, not `cargo run --release`).

For production comparisons, run with `--release`:

```sh
cargo run --release --example benchmarks
```

---

## Baseline: Godot 4.6.1-stable

- **Oracle pin**: `4.6.1-stable` (`14d19694e0c88a3f9e82d899a0400f27a24c176e`)
- **Recorded**: 2026-03-20
- **Bead**: pat-ad8q (Update benchmark baselines after 4.6.1 repin)
- **Build profile**: debug (`cargo run --example benchmarks`)
- **Machine**: darwin 25.3.0 (Apple Silicon — exact hardware unrecorded; treat as relative baseline only)
- **Iterations per benchmark**: 100

| Benchmark | Total µs | Avg µs/iter | Notes |
|-----------|----------|-------------|-------|
| `scene_load` | 1,976 | 19 | `demo_2d.tscn` → `PackedScene::from_tscn` |
| `resource_load` | 1,895 | 18 | Small `.tres` resource via `TresLoader` |
| `physics_step_2d` | 11,424,694 | 114,246 | 100 rigid bodies × 60 frames, 2D world |
| `physics_step_3d` | 10,882,758 | 108,827 | 100 rigid bodies × 60 frames, 3D world |
| `variant_conversion` | 101,959 | 1,019 | 10 variant types × 100 JSON roundtrips |
| `render_frame_2d` | 147,507 | 1,475 | 100 canvas items (50 rects, 50 circles), 640×480 |

### Notes

- Physics benchmarks are the dominant cost: each iteration constructs a fresh world,
  adds 100 bodies, then steps 60 frames. World-construction overhead is included.
- Scene and resource load benchmarks are fast because the input fixtures are small.
  Scale will increase as fixture complexity grows.
- Render benchmark uses the software rasterizer — no GPU, no OS windowing.

---

## Historical: Godot 4.5.1-stable (pre-repin)

- **Oracle pin**: `4.5.1-stable` (`f62fdbde15035c5576dad93e586201f4d41ef0cb`)
- **Status**: PRE-REPIN — these numbers were never formally recorded in a baseline
  document before the upstream pin was advanced to 4.6.1. The benchmark harness
  (`engine-rs/examples/benchmarks.rs`) did not change between 4.5.1 and 4.6.1 pins,
  so the 4.6.1 numbers above are the first formally recorded baseline.
- **Action required**: None. The oracle pin has moved. Re-run benchmarks after any
  significant engine change and record updated numbers in a new "Baseline" section below.

---

## Regeneration Instructions

When the upstream oracle pin advances again:

1. Update `upstream/godot` submodule to the new tag.
2. Run `cd engine-rs && cargo run --example benchmarks` (debug) and optionally
   `cargo run --release --example benchmarks` (release).
3. Add a new "Baseline: Godot X.Y.Z-stable" section to this file with the results.
4. Note the previous baseline as "Historical" and preserve it for regression tracking.
5. Commit referencing the bead that drove the repin.

---

## Benchmark Coverage Gaps

The following subsystems are not yet covered by the benchmark harness and should
be added as the engine matures:

| Subsystem | Why Not Yet |
|-----------|------------|
| GDScript execution | Requires representative script corpus |
| Signal dispatch (bulk) | No high-volume signal benchmark fixture yet |
| Scene instancing (deep hierarchy) | Large fixture not yet in `fixtures/scenes/` |
| Audio mixing | Audio is stub-only; no runtime to benchmark |
| 3D physics (broad phase) | 3D world is young; shape variety is limited |
